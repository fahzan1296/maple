//! Usage display helpers shared by usage overlays.
//!
//! Reference: `l/src/overlays/usage-display.ts`.
//!
//! Deviations:
//! - `UsageLimit`, `UsageReport`, `UsageResetCredits`, and related AI types from
//!   `@oh-my-pi/pi-ai` have no crate equivalent. This port defines structural
//!   data models carrying the fields used by the reference helpers.
//! - Reset credit expiry is modeled as epoch milliseconds (`i64`).
//! - `soonest_expiry` in [`UsageResetSummary`] is stored as epoch milliseconds
//!   (`Option<i64>`) rather than a formatted string; formatting is a display
//!   concern for callers.

use std::collections::HashSet;

/// Scope metadata for a usage limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageLimitScope {
	pub tier: Option<String>,
	pub shared: Option<bool>,
	pub window_id: Option<String>,
}

/// Window metadata for a usage limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageLimitWindow {
	pub id: Option<String>,
	pub label: Option<String>,
	pub duration_ms: Option<u64>,
}

/// A provider usage/quota limit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageLimit {
	pub id: Option<String>,
	pub label: String,
	pub scope: UsageLimitScope,
	pub window: Option<UsageLimitWindow>,
	pub status: Option<String>,
}

/// Reset credit detail.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageResetCreditDetail {
	pub expires_at_ms: Option<i64>,
	pub redeemable: Option<bool>,
}

/// Reset credits metadata attached to a usage report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageResetCredits {
	pub available_count: Option<u32>,
	pub redeemable_count: Option<u32>,
	pub credits: Option<Vec<UsageResetCreditDetail>>,
	pub unavailable_reason: Option<String>,
}

/// A provider usage report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageReport {
	pub provider: String,
	pub limits: Vec<UsageLimit>,
	pub reset_credits: Option<UsageResetCredits>,
}

/// Include the usage tier in a limit title unless its label already names it.
#[must_use]
pub fn format_limit_title(limit: &UsageLimit) -> String {
	if let Some(tier) = &limit.scope.tier {
		if !limit.label.to_lowercase().contains(&tier.to_lowercase()) {
			return format!("{} ({tier})", limit.label);
		}
	}
	limit.label.clone()
}

fn collapse_shared_limits(limits: &[UsageLimit]) -> Vec<UsageLimit> {
	let mut seen_groups: HashSet<String> = HashSet::new();
	let mut collapsed: Option<Vec<UsageLimit>> = None;

	for (index, limit) in limits.iter().enumerate() {
		let Some(window_id) = &limit.scope.window_id else {
			if let Some(out) = &mut collapsed {
				out.push(limit.clone());
			}
			continue;
		};
		if limit.scope.shared != Some(true) {
			if let Some(out) = &mut collapsed {
				out.push(limit.clone());
			}
			continue;
		}
		if !seen_groups.insert(window_id.clone()) {
			if collapsed.is_none() {
				collapsed = Some(limits[..index].to_vec());
			}
			continue;
		}
		if let Some(out) = &mut collapsed {
			out.push(limit.clone());
		}
	}

	collapsed.unwrap_or_else(|| limits.to_vec())
}

/// Human-readable quota scope for a normalized saved-reset window ID.
#[must_use]
pub fn format_usage_reset_window(window_id: &str) -> String {
	match window_id {
		"five_hour" => "5-hour quota".to_string(),
		"seven_day" => "weekly quota".to_string(),
		"seven_day_opus" => "Opus quota".to_string(),
		"seven_day_sonnet" => "Sonnet quota".to_string(),
		"seven_day_cowork" => "Cowork quota".to_string(),
		other => other.replace('_', " "),
	}
}

/// Saved inventory and current eligibility shared by every usage display.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageResetSummary {
	pub banked_count: u32,
	pub redeemable_count: u32,
	pub soonest_expiry_ms: Option<i64>,
	pub unavailable_reason: Option<String>,
}

/// Expiry (epoch ms) of a credit that still banks a reset.
#[must_use]
pub fn banked_reset_credit_expiry_ms(credit: &UsageResetCreditDetail) -> Option<i64> {
	credit.expires_at_ms
}

/// Normalize old count-only and current provider reset metadata for display.
#[must_use]
pub fn summarize_usage_reset_credits(
	reset: Option<&UsageResetCredits>,
	now_ms: i64,
) -> Option<UsageResetSummary> {
	let reset = reset?;
	let credits = reset.credits.as_deref().unwrap_or(&[]);
	if credits.is_empty() && reset.available_count.is_none() && reset.redeemable_count.is_none() {
		return reset.unavailable_reason.as_ref().map(|reason| UsageResetSummary {
			banked_count: 0,
			redeemable_count: 0,
			soonest_expiry_ms: None,
			unavailable_reason: Some(reason.clone()),
		});
	}

	let mut banked_count = 0u32;
	let mut redeemable_count = 0u32;
	let mut soonest_expiry: Option<i64> = None;

	if !credits.is_empty() {
		for credit in credits {
			let Some(expiry) = banked_reset_credit_expiry_ms(credit) else {
				continue;
			};
			if expiry <= now_ms {
				continue;
			}
			banked_count += 1;
			soonest_expiry = Some(match soonest_expiry {
				Some(prev) => prev.min(expiry),
				None => expiry,
			});
			if credit.redeemable.unwrap_or(true) {
				redeemable_count += 1;
			}
		}
	} else {
		banked_count = reset.available_count.unwrap_or(0);
		redeemable_count = reset.redeemable_count.unwrap_or(banked_count);
	}

	if banked_count == 0 && redeemable_count == 0 {
		if let Some(reason) = &reset.unavailable_reason {
			return Some(UsageResetSummary {
				banked_count,
				redeemable_count,
				soonest_expiry_ms: None,
				unavailable_reason: Some(reason.clone()),
			});
		}
		return Some(UsageResetSummary {
			banked_count,
			redeemable_count,
			soonest_expiry_ms: None,
			unavailable_reason: None,
		});
	}

	Some(UsageResetSummary {
		banked_count,
		redeemable_count,
		soonest_expiry_ms: soonest_expiry,
		unavailable_reason: reset.unavailable_reason.clone(),
	})
}

/// Collapse routing-specific copies of a shared quota for user-facing usage views.
#[must_use]
pub fn collapse_shared_usage_reports(reports: &[UsageReport]) -> Vec<UsageReport> {
	let mut collapsed: Option<Vec<UsageReport>> = None;
	for (index, report) in reports.iter().enumerate() {
		let limits = collapse_shared_limits(&report.limits);
		if limits.len() == report.limits.len() {
			if let Some(out) = &mut collapsed {
				out.push(report.clone());
			}
			continue;
		}
		if collapsed.is_none() {
			collapsed = Some(reports[..index].to_vec());
		}
		if let Some(out) = &mut collapsed {
			out.push(UsageReport {
				provider: report.provider.clone(),
				limits,
				reset_credits: report.reset_credits.clone(),
			});
		}
	}
	collapsed.unwrap_or_else(|| reports.to_vec())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn format_limit_title_includes_tier_when_missing() {
		let limit = UsageLimit {
			label: "Requests".into(),
			scope: UsageLimitScope {
				tier: Some("Pro".into()),
				..Default::default()
			},
			..Default::default()
		};
		assert_eq!(format_limit_title(&limit), "Requests (Pro)");
	}

	#[test]
	fn format_limit_title_skips_duplicate_tier() {
		let limit = UsageLimit {
			label: "Pro Requests".into(),
			scope: UsageLimitScope {
				tier: Some("Pro".into()),
				..Default::default()
			},
			..Default::default()
		};
		assert_eq!(format_limit_title(&limit), "Pro Requests");
	}

	#[test]
	fn format_usage_reset_window_maps_known_ids() {
		assert_eq!(format_usage_reset_window("five_hour"), "5-hour quota");
		assert_eq!(format_usage_reset_window("seven_day"), "weekly quota");
		assert_eq!(format_usage_reset_window("custom_window"), "custom window");
	}

	#[test]
	fn summarize_usage_reset_credits_counts_unexpired_redeemable() {
		let reset = UsageResetCredits {
			credits: Some(vec![
				UsageResetCreditDetail { expires_at_ms: Some(200), redeemable: Some(true) },
				UsageResetCreditDetail { expires_at_ms: Some(150), redeemable: Some(false) },
				UsageResetCreditDetail { expires_at_ms: Some(50), redeemable: Some(true) },
			]),
			..Default::default()
		};
		let s = summarize_usage_reset_credits(Some(&reset), 100).unwrap();
		assert_eq!(s.banked_count, 2);
		assert_eq!(s.redeemable_count, 1);
		assert_eq!(s.soonest_expiry_ms, Some(150));
	}

	#[test]
	fn collapse_shared_limits_keeps_first_shared_window_group() {
		let limits = vec![
			UsageLimit {
				label: "A".into(),
				scope: UsageLimitScope {
					shared: Some(true),
					window_id: Some("w".into()),
					..Default::default()
				},
				..Default::default()
			},
			UsageLimit {
				label: "B".into(),
				scope: UsageLimitScope {
					shared: Some(true),
					window_id: Some("w".into()),
					..Default::default()
				},
				..Default::default()
			},
			UsageLimit { label: "C".into(), ..Default::default() },
		];
		let collapsed = collapse_shared_limits(&limits);
		assert_eq!(collapsed.len(), 2);
		assert_eq!(collapsed[0].label, "A");
		assert_eq!(collapsed[1].label, "C");
	}

	#[test]
	fn collapse_shared_usage_reports_only_clones_when_changed() {
		let reports = vec![UsageReport {
			provider: "p".into(),
			limits: vec![UsageLimit { label: "x".into(), ..Default::default() }],
			reset_credits: None,
		}];
		let out = collapse_shared_usage_reports(&reports);
		assert_eq!(out.len(), 1);
		assert_eq!(out[0].provider, "p");
	}
}
