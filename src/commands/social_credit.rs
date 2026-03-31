use crate::{Context, Result, config, db, entity};
use color_eyre::eyre::{ContextCompat, eyre};
use image::{ImageReader, RgbaImage};
use imagetext::prelude::*;
use itertools::Itertools;
use log::warn;
use mc_rcon::RconClient;
use poise::CreateReply;
use poise::serenity_prelude::{
	CreateAllowedMentions, CreateAttachment, Member, Mentionable, RoleId, small_fixed_array::FixedArray,
};
use sea_orm::{ActiveModelTrait, ActiveValue, EntityTrait, IntoActiveModel, QueryOrder, QuerySelect};
use serenity::all::{CreateEmbed, CreateEmbedFooter, UserId};
use std::io::Cursor;
use std::sync::{Arc, LazyLock};

static ROLE_LIMITS: LazyLock<[(RoleId, u64); 6]> = LazyLock::new(|| {
	let config = config::get();
	let mut limits = [
		(config.patron_tier_5_role, 10000),
		(config.patron_tier_4_role, 1500),
		(config.patron_tier_3_role, 500),
		(config.patron_tier_2_role, 200),
		(config.patron_tier_1_role, 100),
		(config.friend_role, 20),
	];
	// a little pointless but just to be sure
	limits.sort_by(|a, b| Ord::cmp(&a.1, &b.1));
	limits.reverse();
	limits
});

static MINUS_CREDIT_TEMPLATE: LazyLock<RgbaImage> = LazyLock::new(|| {
	ImageReader::open("assets/images/minus_credit_template.png")
		.expect("Image should open successfully")
		.decode()
		.expect("Image should decode successfully")
		.into_rgba8()
});

static PLUS_CREDIT_TEMPLATE: LazyLock<RgbaImage> = LazyLock::new(|| {
	ImageReader::open("assets/images/plus_credit_template.png")
		.expect("Image should open successfully")
		.decode()
		.expect("Image should decode successfully")
		.into_rgba8()
});

static ARIAL: LazyLock<SuperFont> = LazyLock::new(|| {
	SuperFont::new(load_font("assets/fonts/arialbd.ttf").expect("Failure to load Arial font"), vec![])
});

static CALIBRI: LazyLock<SuperFont> = LazyLock::new(|| {
	SuperFont::new(load_font("assets/fonts/calibri.ttf").expect("Failure to load Calibri font"), vec![])
});

#[allow(clippy::unused_async)]
#[poise::command(slash_command, subcommands("target", "leaderboard", "leaderboard_public"))]
pub async fn social_credit(_ctx: Context<'_>) -> Result<()> {
	Ok(())
}

#[allow(clippy::unreadable_literal)]
#[poise::command(
	slash_command,
	description_localized("en-US", "Enforce our communal social credit system ☭"),
	member_cooldown = 300
)]
pub async fn target(
	ctx: Context<'_>,
	#[description = "WaveTech Member"] target: Member,
	#[description = "Amount of social credit to add or subtract"] amount: i64,
) -> Result<()> {
	let config = config::get();

	if !has_member_or_provisional(&config, &target.roles) {
		ctx.send(CreateReply::default().content("Target must be a Member or a Provisional!").ephemeral(true)).await?;
		return Ok(());
	}

	let author = &ctx.author_member().await.wrap_err("Could not get author member")?;
	let author_roles = &author.roles;
	if !config.can_members_change_social_credit && has_member_or_provisional(&config, author_roles) {
		ctx.send(CreateReply::default().content("Nuh-uh").ephemeral(true)).await?;
		return Ok(());
	}

	let limit = get_limit(author_roles);
	if amount == 0 || amount.unsigned_abs() > limit {
		let mut message = format!("Invalid amount! Pick an amount between -{limit} and {limit}");
		if amount == 0 {
			message.push_str(" (excluding 0)");
		}
		ctx.send(CreateReply::default().content(message).ephemeral(true)).await?;
	} else {
		// we can defer now, as ephemeral messages are no longer needed
		ctx.defer().await?;

		let db = db();
		let target_id = target.user.id;
		let user = entity::social_credit_user::Entity::find_by_id(target_id)
			.one(db)
			.await?
			.map_or_else(
				|| {
					entity::social_credit_user::ActiveModel {
						id: ActiveValue::Set(i64::from(target_id)),
						social_credit: ActiveValue::Set(amount),
						..Default::default()
					}
					.insert(db)
				},
				|model| {
					let mut active_model = model.into_active_model();
					active_model.social_credit = ActiveValue::Set(active_model.social_credit.unwrap() + amount);
					active_model.update(db)
				},
			)
			.await?;

		ctx.send(
			CreateReply::default()
				.embed(
					CreateEmbed::new()
						.description(format!(
							"### {amount:+} Social Credit to {} from {}",
							target.mention(),
							author.mention()
						))
						.image("attachment://social_credit_image.png")
						.footer(CreateEmbedFooter::new(format!("current social credit: {}", user.social_credit)))
						.color(if amount < 0 { 0xC24A3F } else { 0x2EB33E }),
				)
				.attachment(CreateAttachment::bytes(generate_image(amount)?, "social_credit_image.png"))
				.allowed_mentions(CreateAllowedMentions::new()),
		)
		.await?;

		sync_with_server(&user).await?;
	}

	Ok(())
}

#[poise::command(
	slash_command,
	description_localized("en-US", "View the social credit leaderboard"),
	member_cooldown = 10
)]
pub async fn leaderboard(ctx: Context<'_>) -> Result<()> {
	send_leaderboard_message(&ctx, true).await?;

	Ok(())
}

#[poise::command(
	slash_command,
	description_localized("en-US", "View the leaderboard as a normal message everyone can see"),
	member_cooldown = 300
)]
pub async fn leaderboard_public(ctx: Context<'_>) -> Result<()> {
	send_leaderboard_message(&ctx, false).await?;

	Ok(())
}

async fn send_leaderboard_message(ctx: &Context<'_>, ephemeral: bool) -> Result<()> {
	let db = db();

	let top_5 = entity::social_credit_user::Entity::find()
		.order_by_desc(entity::social_credit_user::Column::SocialCredit)
		.limit(5)
		.all(db)
		.await?;
	let bottom_5 = entity::social_credit_user::Entity::find()
		.order_by_asc(entity::social_credit_user::Column::SocialCredit)
		.limit(5)
		.all(db)
		.await?;

	#[allow(clippy::items_after_statements)]
	fn format_for_leaderboard(users: Vec<entity::social_credit_user::Model>) -> String {
		users
			.into_iter()
			.enumerate()
			.map(|(n, model)| {
				format!("{n}. {}: {}", UserId::new(model.id.cast_unsigned()).mention(), model.social_credit)
			})
			.intersperse("\n".to_string())
			.collect()
	}
	let top_5 = format_for_leaderboard(top_5);
	let bottom_5 = format_for_leaderboard(bottom_5);
	let embed = CreateEmbed::new().description(format!(
		r"
		## Social Credit Leaderboard
		**Top 5**
		{top_5}

		**Bottom 5**
		{bottom_5}
		"
	));

	ctx.send(CreateReply::default().embed(embed).ephemeral(ephemeral)).await?;

	Ok(())
}

#[allow(clippy::cast_possible_truncation)]
async fn sync_with_server(user: &entity::social_credit_user::Model) -> Result<()> {
	let Some(ign) = user.ign.clone() else { return Ok(()) };

	let social_credit = user.social_credit;
	let _ = tokio::task::spawn_blocking(move || -> Result<()> {
		let config = config::get();
		let client = RconClient::connect(format!("{}:{}", config.rcon_ip, config.rcon_port))?;
		client.log_in(&config.rcon_pass)?;
		client.send_command(&format!(
			"scoreboard players set {ign} {} {}",
			config.social_credit_objective, social_credit as i32
		))?;
		Ok(())
	})
	.await?
	.map_err(|err| warn!("Failed to sync social credit with server: {err}"));

	Ok(())
}

fn generate_image(amount: i64) -> Result<Vec<u8>> {
	let mut image = if amount < 0 { MINUS_CREDIT_TEMPLATE.clone() } else { PLUS_CREDIT_TEMPLATE.clone() };

	if amount < 0 {
		draw_text_mut(
			&mut image,
			&WHITE,
			Outline::None,
			166.0,
			76.0,
			scale(50.0),
			&CALIBRI,
			&amount.unsigned_abs().to_string(),
		)
		.map_err(|err| eyre!("Error while drawing text on image: {err}"))?;
	} else {
		draw_text_mut(
			&mut image,
			&WHITE,
			Outline::solid(&stroke(5.0), None),
			200.0,
			88.0,
			scale(52.0),
			&ARIAL,
			&amount.to_string(),
		)
		.map_err(|err| eyre!("Error while drawing text on image: {err}"))?;
	}

	let mut image_bytes: Vec<u8> = Vec::new();
	image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Png)?;
	Ok(image_bytes)
}

fn get_limit(roles: &FixedArray<RoleId>) -> u64 {
	// default limit
	let mut limit = 10;
	for (role, role_limit) in ROLE_LIMITS.iter() {
		if roles.contains(role) {
			limit = *role_limit;
			break;
		}
	}
	limit * 1000
}

fn has_member_or_provisional(config: &Arc<config::Config>, user_roles: &FixedArray<RoleId>) -> bool {
	user_roles.contains(&config.member_role) || user_roles.contains(&config.provisional_role)
}
