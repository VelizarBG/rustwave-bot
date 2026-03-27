use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "social_credit_user")]
pub struct Model {
	#[sea_orm(primary_key, auto_increment = false)]
	pub id: i64,
	pub social_credit: i64,
	pub ign: Option<String>,
}

impl ActiveModelBehavior for ActiveModel {}
