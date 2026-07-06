//! Renders the two-engine "match statistics" card (Elo ± error, LOS, SPRT).
//! Shown only when a tournament has exactly two participants, where these
//! pairwise statistics are meaningful.

use eframe::egui::{RichText, Ui};

use colosseum_core::{EngineId, SprtDecision, Standings, elo_with_error, los, sprt};

use crate::theme;
use crate::widgets;

/// Default SPRT hypotheses: H0 = no gain, H1 = +5 Elo, α = β = 0.05.
const SPRT_ELO0: f64 = 0.0;
const SPRT_ELO1: f64 = 5.0;
const SPRT_ALPHA: f64 = 0.05;
const SPRT_BETA: f64 = 0.05;
/// z for a 95% confidence interval.
const Z_95: f64 = 1.96;

/// Draw the match-statistics card if `participants` holds exactly two engines.
/// `participants` is `(id, name)` in display (rank) order; the first is "A".
pub fn match_stats_card(ui: &mut Ui, participants: &[(EngineId, String)], standings: &Standings) {
    let [(a_id, a_name), (b_id, b_name)] = participants else {
        return;
    };
    let h = standings.head_to_head(*a_id, *b_id);
    if h.games() == 0 {
        return;
    }
    let (w, d, l) = (h.wins, h.draws, h.losses);

    widgets::section_card(
        ui,
        "Match statistics",
        Some(&format!("{a_name} vs {b_name}")),
        |ui| {
            let n = w + d + l;
            let score = f64::from(w) + 0.5 * f64::from(d);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{a_name}: {score:.1} / {n}"))
                        .color(theme::text())
                        .font(theme::semibold(13.0)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("(+{w} ={d} -{l})"))
                        .color(theme::text_weak())
                        .size(12.5),
                );
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                // Elo ± error.
                match elo_with_error(w, d, l, Z_95) {
                    Some(est) => {
                        let c = if est.elo >= 0.0 {
                            theme::success()
                        } else {
                            theme::danger()
                        };
                        stat(ui, "Elo");
                        ui.label(
                            RichText::new(format!("{:+.0} ± {:.0}", est.elo, est.margin()))
                                .color(c)
                                .font(theme::semibold(13.0)),
                        )
                        .on_hover_text(format!(
                            "95% confidence interval: [{:+.0}, {:+.0}]",
                            est.lower, est.upper
                        ));
                    }
                    None => {
                        stat(ui, "Elo");
                        ui.label(RichText::new("—").color(theme::text_faint()).size(13.0))
                            .on_hover_text("Undefined until there is at least one win and one loss.");
                    }
                }

                ui.add_space(16.0);

                // LOS.
                let los_pct = los(w, l) * 100.0;
                stat(ui, "LOS");
                ui.label(
                    RichText::new(format!("{los_pct:.1}%"))
                        .color(theme::text())
                        .font(theme::semibold(13.0)),
                )
                .on_hover_text("Likelihood of superiority: probability A is stronger than B.");

                ui.add_space(16.0);

                // SPRT.
                let r = sprt(w, d, l, SPRT_ELO0, SPRT_ELO1, SPRT_ALPHA, SPRT_BETA);
                let (verdict, vc) = match r.decision {
                    SprtDecision::AcceptH1 => ("H1 accepted", theme::success()),
                    SprtDecision::AcceptH0 => ("H0 accepted", theme::danger()),
                    SprtDecision::Continue => ("continue", theme::text_weak()),
                };
                stat(ui, "SPRT");
                ui.label(
                    RichText::new(format!("LLR {:.2}", r.llr))
                        .color(theme::text())
                        .font(theme::semibold(13.0)),
                )
                .on_hover_text(format!(
                    "H0: {SPRT_ELO0:+.0} Elo  vs  H1: {SPRT_ELO1:+.0} Elo \
                     (α={SPRT_ALPHA}, β={SPRT_BETA})\nbounds [{:.2}, {:.2}]",
                    r.lower, r.upper
                ));
                ui.label(RichText::new(format!("· {verdict}")).color(vc).size(12.5));
            });
        },
    );
}

/// A dim caption label preceding a statistic value.
fn stat(ui: &mut Ui, label: &str) {
    ui.label(RichText::new(label).color(theme::text_faint()).size(12.0));
    ui.add_space(2.0);
}
