use crate::actor_store::aws::s3::S3BlobStore;
use crate::actor_store::ActorStore;
use crate::apis::ApiError;
use crate::auth_verifier::AccessFullImport;
use crate::db::DbConn;
use crate::repo::prepare::{
    prepare_create, prepare_delete, prepare_update, PrepareCreateOpts, PrepareDeleteOpts,
    PrepareUpdateOpts,
};
use anyhow::Result;
use aws_config::SdkConfig;
use futures::{stream, StreamExt};
use lexicon_cid::Cid;
use rocket::data::ToByteUnit;
use rocket::data::{FromData, Outcome};
use rocket::http::{ContentType, Status};
use rocket::request::Request;
use rocket::{Data, State};
use rsky_common::env::env_int;
use rsky_repo::block_map::BlockMap;
use rsky_repo::car::{read_stream_car_with_root, CarWithRoot};
use rsky_repo::parse::get_and_parse_record;
use rsky_repo::repo::Repo;
use rsky_repo::sync::consumer::{verify_diff, VerifyRepoInput};
use rsky_repo::types::{PreparedWrite, RecordWriteDescript, VerifiedDiff};
use std::num::NonZeroU64;
use std::ops::{Deref, DerefMut};

const DEFAULT_IMPORT_LIMIT: usize = 100;

#[derive(Debug)]
pub enum CarError {
    ContentLengthMissing,
    ContentLengthInvalid,
    ContentLengthTooLarge,
    InvalidContentType,
    ImportError(String),
}

// Wrapper struct that we can implement FromData for
pub struct CarWithRootWrapper(pub CarWithRoot);

// Implement Deref for ergonomic access to inner CarWithRoot
impl Deref for CarWithRootWrapper {
    type Target = CarWithRoot;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// Implement DerefMut for mutable access to inner CarWithRoot
impl DerefMut for CarWithRootWrapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Implement FromData for our wrapper instead of directly for CarWithRoot
#[rocket::async_trait]
impl<'r> FromData<'r> for CarWithRootWrapper {
    type Error = CarError;

    async fn from_data(req: &'r Request<'_>, data: Data<'r>) -> Outcome<'r, Self> {
        // Check Content-Type
        if let Some(content_type) = req.content_type() {
            if content_type != &ContentType::new("application", "vnd.ipld.car") {
                return Outcome::Error((
                    Status::UnsupportedMediaType,
                    CarError::InvalidContentType,
                ));
            }
        }

        // Get and validate Content-Length
        let content_length = match req.headers().get_one("content-length") {
            None => {
                return Outcome::Error((Status::LengthRequired, CarError::ContentLengthMissing));
            }
            Some(len) => match len.parse::<NonZeroU64>() {
                Ok(len) => len.get(),
                Err(_) => {
                    return Outcome::Error((Status::BadRequest, CarError::ContentLengthInvalid));
                }
            },
        };

        // Get import size limit from env or use default
        let import_limit = env_int("IMPORT_REPO_LIMIT")
            .unwrap_or(DEFAULT_IMPORT_LIMIT)
            .megabytes();

        // Validate against size limit
        if content_length > import_limit.as_u64() {
            return Outcome::Error((Status::PayloadTooLarge, CarError::ContentLengthTooLarge));
        }

        // Limit the data stream to the exact content length
        let limited_stream = data.open(content_length.bytes());

        // Parse the CAR file
        match read_stream_car_with_root(limited_stream).await {
            Ok(car) => Outcome::Success(CarWithRootWrapper(car)),
            Err(e) => Outcome::Error((Status::BadRequest, CarError::ImportError(e.to_string()))),
        }
    }
}

#[tracing::instrument(skip_all)]
#[rocket::post("/xrpc/com.atproto.repo.importRepo", data = "<car_with_root>")]
pub async fn import_repo(
    auth: AccessFullImport,
    mut car_with_root: CarWithRootWrapper,
    s3_config: &State<SdkConfig>,
    db: DbConn,
) -> Result<(), ApiError> {
    let requester = auth.access.credentials.unwrap().did.unwrap();
    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
        db,
    );

    // Get current repo if it exists
    let curr_root: Option<Cid> = actor_store.get_repo_root().await;
    let curr_repo: Option<Repo> = match curr_root {
        None => None,
        Some(_root) => Some(Repo::load(actor_store.storage.clone(), curr_root).await?),
    };

    // Get verified difference from current repo and imported repo
    let imported_root: Cid = car_with_root.root;
    let imported_blocks = &mut car_with_root.blocks;
    let opts = VerifyRepoInput {
        ensure_leaves: Some(false),
    };

    let diff: VerifiedDiff = match verify_diff(
        curr_repo,
        imported_blocks,
        imported_root,
        None,
        None,
        Some(opts),
    )
    .await
    {
        Ok(res) => res,
        Err(error) => {
            tracing::error!("{:?}", error);
            return Err(ApiError::RuntimeError);
        }
    };

    let commit_data = diff.commit;
    let prepared_writes: Vec<PreparedWrite> =
        prepare_import_repo_writes(requester, diff.writes, &imported_blocks).await?;
    match actor_store
        .process_import_repo(commit_data, prepared_writes)
        .await
    {
        Ok(_res) => {}
        Err(error) => {
            tracing::error!("Error importing repo\n{error}");
            return Err(ApiError::RuntimeError);
        }
    }

    Ok(())
}

/// Converts list of RecordWriteDescripts into a list of PreparedWrites
async fn prepare_import_repo_writes(
    _did: String,
    writes: Vec<RecordWriteDescript>,
    blocks: &BlockMap,
) -> Result<Vec<PreparedWrite>, ApiError> {
    match stream::iter(writes)
        .then(|write| {
            let did = _did.clone();
            async move {
                Ok::<PreparedWrite, anyhow::Error>(match write {
                    RecordWriteDescript::Create(write) => {
                        let parsed_record = get_and_parse_record(blocks, write.cid)?;
                        PreparedWrite::Create(
                            prepare_create(PrepareCreateOpts {
                                did: did.clone(),
                                collection: write.collection,
                                rkey: Some(write.rkey),
                                swap_cid: None,
                                record: parsed_record.record,
                                validate: Some(true),
                            })
                            .await?,
                        )
                    }
                    RecordWriteDescript::Update(write) => {
                        let parsed_record = get_and_parse_record(blocks, write.cid)?;
                        PreparedWrite::Update(
                            prepare_update(PrepareUpdateOpts {
                                did: did.clone(),
                                collection: write.collection,
                                rkey: write.rkey,
                                swap_cid: None,
                                record: parsed_record.record,
                                validate: Some(true),
                            })
                            .await?,
                        )
                    }
                    RecordWriteDescript::Delete(write) => {
                        PreparedWrite::Delete(prepare_delete(PrepareDeleteOpts {
                            did: did.clone(),
                            collection: write.collection,
                            rkey: write.rkey,
                            swap_cid: None,
                        })?)
                    }
                })
            }
        })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<PreparedWrite>, _>>()
    {
        Ok(res) => Ok(res),
        Err(error) => {
            tracing::error!("Error preparing import repo writes\n{error}");
            Err(ApiError::RuntimeError)
        }
    }
}
