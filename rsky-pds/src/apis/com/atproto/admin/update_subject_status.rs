use crate::account_manager::AccountManager;
use crate::auth_verifier::Moderator;
use crate::models::{ErrorCode, ErrorMessageResponse};
use crate::repo::aws::s3::S3BlobStore;
use crate::repo::ActorStore;
use crate::SharedSequencer;
use anyhow::Result;
use aws_config::SdkConfig;
use libipld::Cid;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::State;
use rsky_lexicon::com::atproto::admin::{Subject, SubjectStatus, UpdateSubjectStatusOutput};
use rsky_syntax::aturi::AtUri;
use std::str::FromStr;

async fn inner_update_subject_status(
    body: Json<SubjectStatus>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
) -> Result<UpdateSubjectStatusOutput> {
    let SubjectStatus {
        subject,
        takedown,
        deactivated,
    } = body.into_inner();

    if let Some(takedown) = &takedown {
        match &subject {
            Subject::RepoRef(subject) => {
                AccountManager::takedown_account(&subject.did, takedown.clone()).await?;
            }
            Subject::StrongRef(subject) => {
                let subject_at_uri: AtUri = subject.uri.clone().try_into()?;
                let actor_store = ActorStore::new(
                    subject_at_uri.get_hostname().to_string(),
                    S3BlobStore::new(subject_at_uri.get_hostname().to_string(), s3_config),
                );
                actor_store
                    .record
                    .update_record_takedown_status(&subject_at_uri, takedown.clone())
                    .await?;
            }
            Subject::RepoBlobRef(subject) => {
                let actor_store = ActorStore::new(
                    subject.did.clone(),
                    S3BlobStore::new(subject.did.clone(), s3_config),
                );
                actor_store
                    .blob
                    .update_blob_takedown_status(Cid::from_str(&subject.cid)?, takedown.clone())
                    .await?;
            }
        }
    }

    if let Some(deactivated) = deactivated {
        if let Subject::RepoRef(subject) = &subject {
            if deactivated.applied {
                AccountManager::deactivate_account(&subject.did, None).await?;
            } else {
                AccountManager::activate_account(&subject.did).await?;
            }
        }
    }

    if let Subject::RepoRef(subject) = &subject {
        let status = AccountManager::get_account_status(&subject.did).await?;
        let mut lock = sequencer.sequencer.write().await;
        lock.sequence_account_evt(subject.did.clone(), status)
            .await?;
    }

    Ok(UpdateSubjectStatusOutput { subject, takedown })
}

#[rocket::post(
    "/xrpc/com.atproto.admin.updateSubjectStatus",
    format = "json",
    data = "<body>"
)]
pub async fn update_subject_status(
    body: Json<SubjectStatus>,
    sequencer: &State<SharedSequencer>,
    s3_config: &State<SdkConfig>,
    _auth: Moderator,
) -> Result<Json<UpdateSubjectStatusOutput>, status::Custom<Json<ErrorMessageResponse>>> {
    match inner_update_subject_status(body, sequencer, s3_config).await {
        Ok(res) => Ok(Json(res)),
        Err(error) => {
            eprintln!("@LOG: ERROR: {error}");
            let internal_error = ErrorMessageResponse {
                code: Some(ErrorCode::InternalServerError),
                message: Some(error.to_string()),
            };
            return Err(status::Custom(
                Status::InternalServerError,
                Json(internal_error),
            ));
        }
    }
}
