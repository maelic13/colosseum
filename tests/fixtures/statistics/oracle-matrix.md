# Statistics oracle matrix

The cell entries are binding for fixture comparisons. `Compare` means compare
the named field only after the fixture’s model, input unit and conditions match.
`Analytic only` means Colosseum’s hand-derived fixture is the oracle. `Never`
means a value may be preserved as diagnostic output but must not be asserted as
statistical parity.

| Field / behaviour | Analytic fixture | Fastchess | cutechess-cli | Rule |
|---|---|---|---|---|
| Pair binning and official pair count | Compare | Compare when complete opening pairs are identifiable | Compare when complete opening pairs are identifiable | Pair identity is required; never infer it from aggregate W/D/L |
| Pentanomial mean, variance, SE | Compare | Compare only where the release prints the same pentanomial definition | Analytic only | The five-bin definitions are not guaranteed by cutechess-cli output |
| Normalized-Elo pentanomial SPRT LLR and Wald bounds | Compare | Compare when `model=normalized` and hypotheses/error rates match | Analytic only | Fastchess is the compatible external surface |
| Logistic-Elo pentanomial SPRT LLR and Wald bounds | Compare | Compare when `model=logistic` and hypotheses/error rates match | Analytic only | Do not substitute a trinomial result for this model |
| Trinomial logistic SPRT LLR and Wald bounds | Compare | Compare only with a matched draw-rate convention | Compare only with a matched draw-rate convention | Compare shared model fields, not runner presentation text |
| W/D/L score and draw ratio | Compare | Compare | Compare | Inputs must have the same completed-game sample |
| Logistic Elo / interval / LOS | Compare | Compare only if score, confidence convention and sample are non-degenerate | Compare only if score, confidence convention and sample are non-degenerate | A clean sweep is an error in Colosseum, not an infinity oracle |
| Normalized-Elo interval and achieved fixed-N resolution | Compare | Analytic only | Analytic only | These are Colosseum’s explicit fixed-N definitions |
| Fixed-N prospective plan | Compare | Analytic only | Analytic only | Assumed five-bin distribution and test objective are required inputs |
| Unpaired-game exclusion | Compare | Never | Never | External aggregate logs normally lack enough opening identity |
| Typed invalid-input and `NaN`/`Inf` prevention | Compare | Never | Never | External behaviour is forensic evidence only |

The matrix deliberately has no “closest” or “approximately similar” rule.
Unsupported fields are excluded, not silently approximated. Phase 1.9 must
list every fixture/matrix cell it executes and fail on an unexplained mismatch.
