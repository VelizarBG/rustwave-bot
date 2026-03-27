mod commands;
mod config;
mod entity;

use crate::commands::reload_config::reload_config;
use crate::commands::social_credit::social_credit;
use flexi_logger::{Cleanup, Criterion, DeferredNow, Duplicate, FileSpec, Naming, style};
use log::{Record, error, info, warn};
use poise::{Command, CreateReply, Framework, FrameworkError, FrameworkOptions};
use sea_orm::{ConnectOptions, Database, DatabaseConnection, EntityTrait};
use serenity::all::{CreateAllowedMentions, EventHandler, FullEvent, GatewayIntents, TransportCompression};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

type Data = ();
type Error = color_eyre::Report;
type Context<'a> = poise::Context<'a, Data, Error>;
type Result<T> = color_eyre::eyre::Result<T, Error>;

static DB: OnceLock<DatabaseConnection> = OnceLock::new();

/// # Panics
///
/// Will panic if DB is not initialized
pub fn db() -> &'static DatabaseConnection {
	DB.get().expect("DB should be initialized")
}

struct Handler;

#[serenity::async_trait]
impl EventHandler for Handler {
	async fn dispatch(&self, ctx: &poise::serenity_prelude::Context, event: &FullEvent) {
		if let FullEvent::Ready { .. } = event {
			info!("Bot is ready!");

			match poise::builtins::register_in_guild(&ctx.http, &commands(), config::get().guild_id).await {
				Ok(()) => info!("Registered commands successfully!"),
				Err(err) => warn!("Could not register all commands! {err:?}"),
			}
		}
	}
}

#[tokio::main]
async fn main() -> Result<()> {
	color_eyre::install()?;

	init_logger()?;

	init_db().await?;

	let config = config::get();

	let framework_options = FrameworkOptions {
		commands: commands(),
		skip_checks_for_owners: true,
		owners: config.owners.clone().into_iter().collect(),
		on_error: |error| Box::pin(on_error(error)),
		..Default::default()
	};

	// Create a new instance of the Client, logging in as a bot. This will automatically prepend
	// your bot token with "Bot ", which is a requirement by Discord for bot users.
	let mut client = serenity::Client::builder(config.discord_token.clone(), GatewayIntents::empty())
		.framework(Box::new(Framework::new(framework_options)))
		.compression(TransportCompression::Zstd)
		.event_handler(Arc::new(Handler))
		.await?;

	// Finally, start a single shard, and start listening to events.
	//
	// Shards will automatically attempt to reconnect, and will perform exponential backoff until
	// it reconnects.
	if let Err(why) = client.start().await {
		println!("Client error: {why:?}");
	}

	// Don't run anything here, it likely won't be reached

	Ok(())
}

fn init_logger() -> Result<()> {
	flexi_logger::Logger::try_with_env_or_str("info")?
		.format(format_custom)
		.rotate(Criterion::Size(16 * 1024 * 1024), Naming::Timestamps, Cleanup::KeepCompressedFiles(16))
		.log_to_file(FileSpec::default().directory("logs"))
		.duplicate_to_stdout(Duplicate::All)
		.start()?;
	Ok(())
}

fn format_custom(
	w: &mut dyn std::io::Write,
	now: &mut DeferredNow,
	record: &Record,
) -> std::result::Result<(), std::io::Error> {
	let level = record.level();
	write!(
		w,
		// "[{}] {} [{}] [{}:{}] ",
		"[{}] {} [{}] ",
		style(level).paint(now.format("%Y-%m-%d %H:%M:%S").to_string()),
		style(level).paint(level.to_string()),
		record.module_path().unwrap_or("<unnamed>"),
		/*record.file().unwrap_or("<unnamed>"),
		record.line().unwrap_or(0),*/
	)?;

	write!(w, "{}", style(level).paint(record.args().to_string()))
}

#[allow(clippy::needless_update)]
async fn init_db() -> Result<()> {
	let mut opt = ConnectOptions::new("sqlite://rustwave-bot.db?mode=rwc");
	// let mut opt = ConnectOptions::new("sqlite::memory:");
	opt.sqlx_logging(false) // disable SQLx logging
		.sqlx_logging_level(log::LevelFilter::Info);
	{
		let db = Database::connect(opt).await?;
		db.get_schema_registry("rustwave-bot::entity::*").sync(&db).await?;
		DB.set(db).expect("DB should not be initialized yet");
	};

	let member_init_path = Path::new("members.init");
	if member_init_path.exists() && !member_init_path.is_dir() {
		let contents = tokio::fs::read_to_string(member_init_path).await?;
		let lines = contents.split('\n');
		let mut members: Vec<(i64, &str)> = vec![];
		for line in lines {
			let line = line.trim();
			if line.is_empty() {
				continue;
			}
			let Some(parts) = line.split_once(',') else { continue };
			members.push((i64::from_str(parts.0)?, parts.1));
		}
		let members = members.iter().map(|member| entity::social_credit_user::ActiveModel {
			id: sea_orm::ActiveValue::Set(member.0),
			social_credit: sea_orm::ActiveValue::Set(0),
			ign: sea_orm::ActiveValue::Set(Some(member.1.to_string())),
			..Default::default()
		});
		entity::social_credit_user::Entity::insert_many(members).exec(db()).await?;

		#[cfg(not(debug_assertions))]
		tokio::fs::rename("members.init", "loaded_members.init").await?;
	}

	Ok(())
}

async fn on_error(error: FrameworkError<'_, Data, Error>) {
	match error {
		FrameworkError::Command { error, ctx, .. } => {
			warn!("Error while executing command '{}': {error:?}", ctx.command().qualified_name);

			let mentions = CreateAllowedMentions::new().everyone(false).all_roles(false).all_users(false);
			if let Err(e) = ctx
				.send(
					CreateReply::default()
						.content("An unexpected error occurred while executing the command")
						.allowed_mentions(mentions)
						.ephemeral(true),
				)
				.await
			{
				info!("Error while sending error message: {e:?}");
			}
		}
		_ => {
			if let Err(e) = poise::builtins::on_error(error).await {
				error!("Error while handling error: {e:?}");
			}
		}
	}
}

fn commands() -> Vec<Command<Data, Error>> {
	vec![reload_config(), social_credit()]
}
