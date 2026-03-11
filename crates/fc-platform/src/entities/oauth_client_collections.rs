//! SeaORM Entities: oauth_client junction tables (redirect URIs, origins, grant types, app IDs)

use sea_orm::entity::prelude::*;

// -- Redirect URIs

pub mod redirect_uris {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "oauth_client_redirect_uris")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub oauth_client_id: String,
        #[sea_orm(primary_key)]
        pub redirect_uri: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// -- Allowed origins

pub mod allowed_origins {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "oauth_client_allowed_origins")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub oauth_client_id: String,
        #[sea_orm(primary_key)]
        pub allowed_origin: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// -- Grant types

pub mod grant_types {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "oauth_client_grant_types")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub oauth_client_id: String,
        #[sea_orm(primary_key)]
        pub grant_type: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// -- Application IDs

pub mod application_ids {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "oauth_client_application_ids")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub oauth_client_id: String,
        #[sea_orm(primary_key)]
        pub application_id: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
