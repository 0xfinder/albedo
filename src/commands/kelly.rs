use crate::Context;
use color_eyre::eyre::Result;

/// Calculates wager size using Kelly's Criterion
#[poise::command(slash_command, prefix_command)]
pub async fn kelly(
    ctx: Context<'_>,
    #[description = "win probability %"] win_probability: f32,
    #[description = "decimal odds"] decimal_odds: f32,
    #[description = "bankroll"] bankroll: u32,
    #[description = "kelly adjustment (number to divide by)"] kelly_adjustment: u32,
) -> Result<()> {
    let fraction_of_bankroll_to_wager: f32 =
        (win_probability / 100.0) - ((1.0 - win_probability / 100.0) / (decimal_odds - 1.0));
    let wager_in_units = fraction_of_bankroll_to_wager * bankroll as f32
        / kelly_adjustment as f32
        / (bankroll as f32 / 100.0);
    let response = format!("bet {:.2}u @ {}", wager_in_units, decimal_odds);
    ctx.reply(response).await?;
    Ok(())
}
