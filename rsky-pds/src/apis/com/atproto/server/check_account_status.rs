use crate::account_manager::AccountManager;
use crate::apis::com::atproto::server::is_valid_did_doc_for_service;
use crate::auth_verifier::AccessFull;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use anyhow::Result;
use aws_config::SdkConfig;
use futures::try_join;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::server::CheckAccountStatusOutput;

async fn inner_check_account_status(
    auth: AccessFull,
    s3_config: &State<SdkConfig>,
) -> Result<CheckAccountStatusOutput> {
    let requester = auth.access.credentials.unwrap().did.unwrap();

    let mut actor_store = ActorStore::new(
        requester.clone(),
        S3BlobStore::new(requester.clone(), s3_config),
    );
    let (storage_clone_1, storage_clone_2) = (actor_store.storage.clone(), actor_store.storage.clone());
    let (repo_root_object, repo_blocks_object) = {
        (storage_clone_1.read().unwrap().get_root_detailed(), storage_clone_2.read().unwrap().count_blocks())
    }
    let (repo_root, repo_blocks, indexed_records, imported_blobs, expected_blobs) = try_join!(
        repo_root_object,
        repo_blocks_object,
        actor_store.record.record_count(),
        actor_store.blob.blob_count(),
        actor_store.blob.record_blob_count(),
    )?;

    let (activated, valid_did) = try_join!(
        AccountManager::is_account_activated(&requester),
        is_valid_did_doc_for_service(requester.clone())
    )?;

    Ok(CheckAccountStatusOutput {
        activated,
        valid_did,
        repo_commit: repo_root.cid.to_string(),
        repo_rev: repo_root.rev,
        repo_blocks,
        indexed_records,
        private_state_values: 0,
        expected_blobs,
        imported_blobs,
    })
}

#[rocket::get("/xrpc/com.atproto.server.checkAccountStatus")]
pub async fn check_account_status(
    auth: AccessFull,
    s3_config: &State<SdkConfig>,
) -> Result<Json<CheckAccountStatusOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_check_account_status(auth, s3_config).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("Internal Error: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some("Internal error".to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
