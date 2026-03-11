//! SeaORM Entity: oauth_oidc_login_states

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "oauth_oidc_login_states")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub state: String,
    pub email_domain: String,
    pub identity_provider_id: String,
    pub email_domain_mapping_id: String,
    pub nonce: String,
    pub code_verifier: String,
    pub return_url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_scope: Option<String>,
    pub oauth_state: Option<String>,
    pub oauth_code_challenge: Option<String>,
    pub oauth_code_challenge_method: Option<String>,
    pub oauth_nonce: Option<String>,
    pub interaction_uid: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub expires_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
