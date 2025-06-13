// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2025 1BitSquared <info@1bitsquared.com>
// SPDX-FileContributor: Written by P-Storm <pauldeman@gmail.com>
// SPDX-FileContributor: Modified by Rachel Mant <git@dragonmux.network>

#[cfg(test)]
mod tests
{
    use color_eyre::eyre::Result;
    use bmputil::metadata::structs::Probe;
    use bmputil::probe_identity::{GitVersion, ProbeIdentity, VersionKind, VersionNumber, VersionParts};

    #[test]
    fn extract_native() -> Result<()>
    {
        let res: ProbeIdentity = "Black Magic Probe v2.0.0-rc2".try_into()?;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, VersionNumber::FullVersion(VersionParts {
            major: 2, minor: 0, revision: 0, kind: VersionKind::ReleaseCandidate(2), dirty: false
        }));
        Ok(())
    }

    #[test]
    fn extract_old() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe").try_into()?;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, VersionNumber::Unknown);
        Ok(())
    }

    #[test]
    fn extract_without_parathesis() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe v2.0.0-rc2-65-g221c3031f").try_into()?;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, VersionNumber::FullVersion(VersionParts {
            major: 2, minor: 0, revision: 0, kind: VersionKind::Development(GitVersion {
                release_candidate: Some(2), 65, "g221c3031f"
            }), dirty: false
        }));
        Ok(())
    }

    #[test]
    fn extract_version_only_hash()
    {
        let result: Result<ProbeIdentity> = String::from("Black Magic Probe g221c3031f").try_into();

        assert!(result.is_err());
        if let Err(err) = result {
            assert_eq!(err.to_string(), "Error while parsing version string: Version doesn't start with v, got 'g221c3031f'");
        }
    }

    #[test]
    fn extract_st_link() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe (ST-Link/v2) v1.10.0-1273-g2b1ce9aee").try_into()?;

        assert_eq!(res.variant(), Probe::Stlink);
        assert_eq!(res.version, VersionNumber::FullVersion(VersionParts {
            major: 1, minor: 10, revision: 0, kind: VersionKind::Development(GitVersion {
                release_candidate: Noone, 1273, "g2b1ce9aee"
            }), dirty: false
        }));
        Ok(())
    }

    #[test]
    fn extract_without_closing()
    {
        let result: Result<ProbeIdentity> = "Black Magic Probe (ST-Link".to_string().try_into();

        assert!(result.is_err());
        if let Err(err) = result {
            assert_eq!(err.to_string(), "Error while parsing probe string: Not a matching pair of parenthesis found.");
        }
    }

    #[test]
    fn unknown()
    {
        let result: Result<ProbeIdentity> = String::from("Something (v1.2.3)").try_into();

        assert!(result.is_err());
        if let Err(err) = result {
            assert_eq!(err.to_string(), "Product string doesn't start with 'Black Magic Probe'");
        }
    }
}
