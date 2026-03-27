use crate::{Context, Result, config};
use log::info;

#[poise::command(
	slash_command,
	description_localized("en-US", "Reload the bot's config"),
	required_permissions = "ADMINISTRATOR"
)]
pub async fn reload_config(ctx: Context<'_>) -> Result<()> {
	config::reload()?;
	info!("Reloaded config!");
	ctx.send(poise::CreateReply::default().content("Reloaded config!").ephemeral(true)).await?;
	Ok(())
}
