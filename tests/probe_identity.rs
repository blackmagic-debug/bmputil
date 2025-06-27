// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by P-Storm <pauldeman@gmail.com>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

#[cfg(test)]
mod tests
{
	use bmputil::metadata::structs::Probe;
	use bmputil::probe_identity::{GitVersion, ProbeIdentity, VersionKind, VersionNumber, VersionParts};
	use color_eyre::eyre::Result;

	#[test]
	fn extract_native() -> Result<()>
	{
		let result: ProbeIdentity = "Black Magic Probe v2.0.0-rc2".try_into()?;

		assert_eq!(result.variant(), Some(Probe::Native));
		assert_eq!(
			result.version,
			VersionNumber::FullVersion(VersionParts::from_parts(2, 0, 0, VersionKind::ReleaseCandidate(2), false))
		);
		Ok(())
	}

	#[test]
	fn extract_old() -> Result<()>
	{
		let result: ProbeIdentity = "Black Magic Probe".try_into()?;

		assert_eq!(result.variant(), Some(Probe::Native));
		assert_eq!(result.version, VersionNumber::Unknown);
		Ok(())
	}

	#[test]
	fn extract_without_parathesis() -> Result<()>
	{
		let result: ProbeIdentity = "Black Magic Probe v2.0.0-rc2-65-g221c3031f".try_into()?;

		assert_eq!(result.variant(), Some(Probe::Native));
		assert_eq!(
			result.version,
			VersionNumber::FullVersion(VersionParts::from_parts(
				2,
				0,
				0,
				VersionKind::Development(GitVersion::from_parts(Some(2), 65, "g221c3031f".into())),
				false
			))
		);
		Ok(())
	}

	#[test]
	fn extract_version_only_hash() -> Result<()>
	{
		let result: ProbeIdentity = "Black Magic Probe g221c3031f".try_into()?;

		assert_eq!(result.variant(), Some(Probe::Native));
		assert_eq!(result.version, VersionNumber::GitHash("221c3031f".into()));
		Ok(())
	}

	#[test]
	fn extract_st_link() -> Result<()>
	{
		let result: ProbeIdentity = "Black Magic Probe (ST-Link/v2) v1.10.0-1273-g2b1ce9aee".try_into()?;

		assert_eq!(result.variant(), Some(Probe::Stlink));
		assert_eq!(
			result.version,
			VersionNumber::FullVersion(VersionParts::from_parts(
				1,
				10,
				0,
				VersionKind::Development(GitVersion::from_parts(None, 1273, "g2b1ce9aee".into())),
				false
			))
		);
		Ok(())
	}

	#[test]
	fn extract_without_closing()
	{
		let result: Result<ProbeIdentity> = "Black Magic Probe (ST-Link".try_into();

		assert!(result.is_err());
		if let Err(err) = result {
			assert_eq!(
				err.to_string(),
				"Error while parsing probe string: Not a matching pair of parenthesis found."
			);
		}
	}

	#[test]
	fn unknown()
	{
		let result: Result<ProbeIdentity> = "Something (v1.2.3)".try_into();

		assert!(result.is_err());
		if let Err(err) = result {
			assert_eq!(err.to_string(), "Product string doesn't start with 'Black Magic Probe'");
		}
	}
}
