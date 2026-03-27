use serde::Deserialize;
use serenity::all::{GuildId, RoleId, Token, UserId};
use std::fs::File;
use std::sync::{Arc, OnceLock, RwLock};

fn writable_config() -> &'static RwLock<Arc<Config>> {
	static CONFIG: OnceLock<RwLock<Arc<Config>>> = OnceLock::new();
	CONFIG.get_or_init(|| RwLock::new(Arc::new(Config::load().unwrap())))
}

pub fn get() -> Arc<Config> {
	writable_config().read().unwrap().clone()
}

pub fn reload() -> crate::Result<()> {
	let new_config = Config::load()?;
	*writable_config().write().unwrap() = Arc::new(new_config);
	Ok(())
}

#[derive(Deserialize)]
pub struct Config {
	pub discord_token: Token,
	pub guild_id: GuildId,
	pub owners: Vec<UserId>,
	pub rcon_ip: String,
	pub rcon_port: String,
	pub rcon_pass: String,
	pub can_members_change_social_credit: bool,
	pub social_credit_objective: String,
	pub member_role: RoleId,
	pub provisional_role: RoleId,
	pub friend_role: RoleId,
	pub patron_tier_1_role: RoleId,
	pub patron_tier_2_role: RoleId,
	pub patron_tier_3_role: RoleId,
	pub patron_tier_4_role: RoleId,
	pub patron_tier_5_role: RoleId,
}

impl Config {
	fn load() -> crate::Result<Config> {
		let file = File::open("config.json")?;
		let config: Config = serde_json::from_reader(file)?;
		Ok(config)
	}
}
