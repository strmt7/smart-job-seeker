//! T041 — offer scenarios. Decompose base/fixed installments/bonus/equity/
//! benefits/relocation with original currencies preserved; uncertainty bands
//! and editable assumptions; no conflation of estimated net with payroll
//! quotes. T042 analytics live in analytics.rs.

use serde::{Deserialize, Serialize};
use waypoint_domain::{Amount, AmountBasis, CompensationCategory, PayPeriod};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfferComponent {
    pub label: String,
    pub amount: Amount,
    /// e.g. "signed_offer" | "verbal" | "estimated"
    pub certainty: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfferScenario {
    pub offer_id: String,
    pub components: Vec<OfferComponent>,
    pub relocation_support: Option<Amount>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OfferComparison {
    pub best_by_base: String,
    pub base_values: Vec<(String, i64)>, // minor units
    pub notes: Vec<String>,
}

/// Compare offers on annual base only, with explicit normalization notes.
/// Variable/equity are listed but never summed into a fake total.
pub fn compare_offers(scenarios: &[OfferScenario]) -> Result<OfferComparison, String> {
    let mut notes = vec![];
    let mut base_values = vec![];
    for s in scenarios {
        let base = s
            .components
            .iter()
            .find(|c| c.amount.category == CompensationCategory::Base);
        let Some(base) = base else {
            return Err(format!("offer {} has no base component", s.offer_id));
        };
        if base.amount.period != PayPeriod::Annual {
            notes.push(format!(
                "{}: base is {:?}; annualization must be explicit and is not performed silently",
                s.offer_id, base.amount.period
            ));
        }
        if base.amount.basis == AmountBasis::Net {
            notes.push(format!(
                "{}: base stated NET; gross comparison requires disclosed conversion",
                s.offer_id
            ));
        }
        base_values.push((s.offer_id.clone(), base.amount.min_minor_units.unwrap_or(0)));
        for c in s.components.iter().skip(1) {
            notes.push(format!(
                "{}: '{}' is a separate {} component and is NOT included in the base comparison",
                s.offer_id,
                c.label,
                format!("{:?}", c.amount.category).to_lowercase()
            ));
        }
    }
    let best_by_base = base_values
        .iter()
        .max_by_key(|(_, v)| *v)
        .map(|(id, _)| id.clone())
        .unwrap_or_default();
    Ok(OfferComparison {
        best_by_base,
        base_values,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chf_base(min: i64) -> Amount {
        Amount {
            currency: "CHF".into(),
            basis: AmountBasis::Gross,
            period: PayPeriod::Annual,
            category: CompensationCategory::Base,
            min_minor_units: Some(min),
            max_minor_units: None,
            source: "signed_offer".into(),
        }
    }

    fn offer(id: &str, base: Amount, extra: Vec<OfferComponent>) -> OfferScenario {
        OfferScenario {
            offer_id: id.into(),
            components: {
                let mut v = vec![OfferComponent {
                    label: "Base".into(),
                    amount: base,
                    certainty: "signed_offer".into(),
                }];
                v.extend(extra);
                v
            },
            relocation_support: None,
        }
    }

    #[test]
    fn compares_annual_gross_base_only_and_notes_extras() {
        let bonus = OfferComponent {
            label: "Performance bonus".into(),
            amount: Amount {
                currency: "CHF".into(),
                basis: AmountBasis::Gross,
                period: PayPeriod::Annual,
                category: CompensationCategory::Variable,
                min_minor_units: Some(500_000),
                max_minor_units: None,
                source: "verbal".into(),
            },
            certainty: "verbal".into(),
        };
        let cmp = compare_offers(&[
            offer("A", chf_base(12_000_000), vec![]),
            offer("B", chf_base(11_500_000), vec![bonus]),
        ])
        .unwrap();
        assert_eq!(cmp.best_by_base, "A");
        assert!(cmp.notes.iter().any(|n| n.contains("NOT included")));
    }

    #[test]
    fn monthly_pay_is_flagged_not_silently_annualized() {
        let monthly = Amount {
            currency: "CHF".into(),
            basis: AmountBasis::Gross,
            period: PayPeriod::Monthly,
            category: CompensationCategory::Base,
            min_minor_units: Some(1_000_000),
            max_minor_units: None,
            source: "signed_offer".into(),
        };
        let cmp = compare_offers(&[offer("M", monthly, vec![])]).unwrap();
        assert!(cmp
            .notes
            .iter()
            .any(|n| n.contains("annualization must be explicit")));
    }

    #[test]
    fn net_basis_comparison_requires_disclosure() {
        let net = Amount {
            currency: "CHF".into(),
            basis: AmountBasis::Net,
            period: PayPeriod::Annual,
            category: CompensationCategory::Base,
            min_minor_units: Some(11_000_000),
            max_minor_units: None,
            source: "signed_offer".into(),
        };
        let cmp = compare_offers(&[offer("N", net, vec![])]).unwrap();
        assert!(cmp.notes.iter().any(|n| n.contains("NET")));
    }
}
