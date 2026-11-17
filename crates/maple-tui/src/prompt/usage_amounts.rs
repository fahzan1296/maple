//! Reference: `l/src/prompt/usage-amounts.ts`.
//!
//! Formatting for usage limits that carry an absolute amount but no fraction,
//! shared by the `/usage` report and the fullscreen dashboard so both surfaces
//! agree on what a bucket is worth. Providers report one of two one-sided
//! shapes: spend-to-date with no allowance (`used` only), or a prepaid balance
//! with no total (`remaining` only). Neither can fill a quota bar, so each
//! renders as text instead.
//!
//! Deviations:
//! - `UsageLimit` comes from the AI package in TypeScript; the Rust port carries
//!   the fields these helpers read (`amount.{unit,used,limit,remaining}` and
//!   `scope.shared`) and keeps `unit` a string, matching the theme key
//!   convention of addressing unions by name.
//! - `resolveUsedFraction` is reimplemented here (a fraction exists only when a
//!   positive `limit` and a finite `used` are both present).
//! - `Intl.NumberFormat` compact notation is replaced by a local formatter with
//!   the same `K`/`M`/`B`/`T` thresholds and one fractional digit.

/// The one-sided amount shape a provider reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageAmount {
	/// Unit name: `"percent"`, `"unknown"`, `"usd"`, `"credits"`, `"tokens"`, …
	pub unit: String,
	pub used: Option<f64>,
	pub limit: Option<f64>,
	pub remaining: Option<f64>,
}

/// Scope a limit applies to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageScope {
	/// One account-wide pool observed once per stored credential.
	pub shared: bool,
}

/// One usage bucket.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageLimit {
	pub amount: UsageAmount,
	pub scope: UsageScope,
}

/// The used fraction a bucket can draw, when both `used` and a positive `limit`
/// exist.
pub fn resolve_used_fraction(limit: &UsageLimit) -> Option<f64> {
	let amount = &limit.amount;
	let used = amount.used?;
	let total = amount.limit?;
	if !used.is_finite() || total <= 0.0 {
		return None;
	}
	Some(used / total)
}

fn is_absolute_unit(limit: &UsageLimit) -> bool {
	limit.amount.unit != "percent" && limit.amount.unit != "unknown"
}

/// Spend-to-date bucket: an absolute `used` with no allowance or remainder.
pub fn is_used_only_absolute_amount(limit: &UsageLimit) -> bool {
	let amount = &limit.amount;
	is_absolute_unit(limit)
		&& amount.used.is_some_and(f64::is_finite)
		&& amount.limit.is_none()
		&& amount.remaining.is_none()
		&& resolve_used_fraction(limit).is_none()
}

/// Prepaid-balance bucket: an absolute `remaining` with no spend or allowance.
pub fn is_remaining_only_absolute_amount(limit: &UsageLimit) -> bool {
	let amount = &limit.amount;
	is_absolute_unit(limit)
		&& amount.remaining.is_some_and(f64::is_finite)
		&& amount.limit.is_none()
		&& amount.used.is_none()
		&& resolve_used_fraction(limit).is_none()
}

/// Compact number with at most one fractional digit (`1.5K`, `12M`).
fn compact_number(value: f64) -> String {
	const STEPS: [(f64, &str); 4] = [
		(1e12, "T"),
		(1e9, "B"),
		(1e6, "M"),
		(1e3, "K"),
	];
	let mut scaled = value;
	let mut suffix = "";
	for (divisor, name) in STEPS {
		if scaled.abs() >= divisor {
			scaled /= divisor;
			suffix = name;
			let rounded = (scaled * 10.0).round() / 10.0;
			if rounded.abs() >= 1000.0 {
				continue;
			}
			scaled = rounded;
			return format_scaled(scaled, suffix);
		}
	}
	format_scaled((value * 10.0).round() / 10.0, suffix)
}

fn format_scaled(value: f64, suffix: &str) -> String {
	let rounded = (value * 10.0).round() / 10.0;
	let body = if (rounded - rounded.trunc()).abs() < f64::EPSILON {
		format!("{}", rounded.trunc() as i64)
	} else {
		format!("{rounded:.1}")
	};
	format!("{body}{suffix}")
}

fn format_quantity(value: f64, unit: &str, suffix: &str) -> String {
	if unit == "usd" {
		return format!("${value:.2} {suffix}");
	}
	format!("{} {unit} {suffix}", compact_number(value))
}

/// Total the prepaid headroom a set of limits actually represents.
///
/// A shared limit is one account-wide pool observed once per stored credential,
/// so N keys on the same account yield N identical rows that must collapse to a
/// single value — summing them would claim several times the credits a single
/// request can draw on. Limits without the flag are genuinely per-account and do
/// add up. `None` unless every limit is remaining-only and agrees on a unit.
pub fn total_remaining_only(limits: &[UsageLimit]) -> Option<(f64, String)> {
	let first = limits.first()?;
	if !limits.iter().all(is_remaining_only_absolute_amount) {
		return None;
	}
	let unit = first.amount.unit.clone();
	if !limits.iter().all(|limit| limit.amount.unit == unit) {
		return None;
	}

	let mut total = 0.0;
	let mut shared_max: Option<f64> = None;
	for limit in limits {
		let remaining = limit.amount.remaining.unwrap_or(0.0);
		if limit.scope.shared {
			shared_max = Some(match shared_max {
				Some(current) => current.max(remaining),
				None => remaining,
			});
			continue;
		}
		total += remaining;
	}
	Some((total + shared_max.unwrap_or(0.0), unit))
}

/// `"100 credits left"` for a bucket of remaining-only limits, else `None`.
pub fn format_remaining_only_total(limits: &[UsageLimit]) -> Option<String> {
	let (value, unit) = total_remaining_only(limits)?;
	Some(format_quantity(value, &unit, "left"))
}

/// Text for a bucket no quota bar can draw: spend-to-date or prepaid headroom.
/// Used-only limits report the worst (highest) spend; remaining-only limits go
/// through [`total_remaining_only`] so shared pools are not double-counted.
pub fn format_absolute_only_amount(limits: &[UsageLimit]) -> Option<String> {
	let first = limits.first()?;
	if limits.iter().all(is_used_only_absolute_amount) {
		let unit = first.amount.unit.clone();
		if !limits.iter().all(|limit| limit.amount.unit == unit) {
			return None;
		}
		let used = limits
			.iter()
			.map(|limit| limit.amount.used.unwrap_or(0.0))
			.fold(0.0_f64, f64::max);
		return Some(format_quantity(used, &unit, "used"));
	}
	format_remaining_only_total(limits)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn limit(unit: &str, used: Option<f64>, total: Option<f64>, remaining: Option<f64>) -> UsageLimit {
		UsageLimit {
			amount: UsageAmount {
				unit: unit.to_string(),
				used,
				limit: total,
				remaining,
			},
			scope: UsageScope::default(),
		}
	}

	#[test]
	fn classifies_one_sided_amounts() {
		let used_only = limit("credits", Some(12.0), None, None);
		assert!(is_used_only_absolute_amount(&used_only));
		assert!(!is_remaining_only_absolute_amount(&used_only));

		let remaining_only = limit("credits", None, None, Some(40.0));
		assert!(is_remaining_only_absolute_amount(&remaining_only));
		assert!(!is_used_only_absolute_amount(&remaining_only));

		let cases = [
			limit("percent", Some(0.5), None, None),
			limit("unknown", Some(3.0), None, None),
			limit("credits", Some(3.0), Some(10.0), None),
			limit("credits", Some(3.0), None, Some(7.0)),
		];
		for case in cases {
			assert!(!is_used_only_absolute_amount(&case), "limit {case:?}");
			assert!(!is_remaining_only_absolute_amount(&case), "limit {case:?}");
		}
	}

	#[test]
	fn resolves_used_fraction() {
		assert_eq!(resolve_used_fraction(&limit("usd", Some(2.5), Some(10.0), None)), Some(0.25));
		assert_eq!(resolve_used_fraction(&limit("usd", Some(2.5), Some(0.0), None)), None);
		assert_eq!(resolve_used_fraction(&limit("usd", Some(2.5), None, None)), None);
	}

	#[test]
	fn totals_shared_pools_once() {
		let mut shared_a = limit("credits", None, None, Some(100.0));
		shared_a.scope.shared = true;
		let mut shared_b = limit("credits", None, None, Some(120.0));
		shared_b.scope.shared = true;
		let personal = limit("credits", None, None, Some(5.0));

		let total = total_remaining_only(&[shared_a.clone(), shared_b, personal.clone()]);
		assert_eq!(total, Some((125.0, "credits".to_string())));

		let mixed_units = total_remaining_only(&[personal, limit("usd", None, None, Some(1.0))]);
		assert_eq!(mixed_units, None);

		// Any bucket with a fraction disqualifies the group.
		let with_total = total_remaining_only(&[
			limit("credits", None, None, Some(5.0)),
			limit("credits", Some(1.0), Some(2.0), None),
		]);
		assert_eq!(with_total, None);
		assert_eq!(total_remaining_only(&[]), None);
	}

	#[test]
	fn formats_quantities() {
		let cases: [(f64, &str, &str); 6] = [
			(100.0, "credits", "100 credits left"),
			(1500.0, "tokens", "1.5K tokens left"),
			(2_000_000.0, "tokens", "2M tokens left"),
			(0.5, "credits", "0.5 credits left"),
			(12.345, "usd", "$12.35 left"),
			(999_950.0, "tokens", "1M tokens left"),
		];
		for (value, unit, expected) in cases {
			assert_eq!(format_quantity(value, unit, "left"), expected);
		}
	}

	#[test]
	fn formats_absolute_only_buckets() {
		let used = format_absolute_only_amount(&[
			limit("usd", Some(1.5), None, None),
			limit("usd", Some(2.25), None, None),
		]);
		assert_eq!(used.as_deref(), Some("$2.25 used"));

		let remaining = format_absolute_only_amount(&[limit("credits", None, None, Some(100.0))]);
		assert_eq!(remaining.as_deref(), Some("100 credits left"));

		assert_eq!(format_absolute_only_amount(&[]), None);
		assert_eq!(
			format_absolute_only_amount(&[
				limit("usd", Some(1.0), None, None),
				limit("credits", Some(1.0), None, None)
			]),
			None
		);
		assert_eq!(format_remaining_only_total(&[limit("credits", Some(1.0), None, None)]), None);
	}
}
