use crate::apis::ApiError;
use rocket::serde::json::Json;
use rsky_common::env::{env_bool, env_list, env_str};
use rsky_lexicon::com::atproto::server::{
    DescribeServerOutput, DescribeServerRefContact, DescribeServerRefLinks,
};

#[tracing::instrument(skip_all)]
#[rocket::get("/xrpc/com.atproto.server.describeServer")]
pub async fn describe_server() -> Result<Json<DescribeServerOutput>, ApiError> {
    let available_user_domains = env_list("PDS_SERVICE_HANDLE_DOMAINS");
    let invite_code_required = env_bool("PDS_INVITE_REQUIRED");
    let privacy_policy = env_str("PDS_PRIVACY_POLICY_URL");
    let terms_of_service = env_str("PDS_TERMS_OF_SERVICE_URL");
    let contact_email_address = env_str("PDS_CONTACT_EMAIL_ADDRESS");

    Ok(Json(DescribeServerOutput {
        did: env_str("PDS_SERVICE_DID").unwrap(),
        available_user_domains,
        invite_code_required,
        phone_verification_required: None,
        links: DescribeServerRefLinks {
            privacy_policy,
            terms_of_service,
        },
        contact: DescribeServerRefContact {
            email: contact_email_address,
        },
    }))
}
