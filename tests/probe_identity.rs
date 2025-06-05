#[cfg(test)]
mod tests {
    use color_eyre::eyre::{eyre, Result};
    use bmputil::metadata::structs::Probe;
    use bmputil::probe_identity::{ProbeIdentity, Version};
    
    #[test]
    fn extract_native() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe v2.0.0-rc2").try_into()?            ;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, Version::Known(String::from("v2.0.0-rc2")));
        Ok(())
    }

    #[test]
    fn extract_old() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe").try_into()?;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, Version::Unknown);
        Ok(())
    }

    #[test]
    fn extract_without_parathesis() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe v2.0.0-rc2-65-g221c3031f").try_into()?;

        assert_eq!(res.variant(), Probe::Native);
        assert_eq!(res.version, Version::Known(String::from("v2.0.0-rc2-65-g221c3031f")));
        Ok(())
    }

    #[test]
    fn extract_version_only_hash_error()
    {
        let res: Result<ProbeIdentity> = String::from("Black Magic Probe g221c3031f").try_into();
        
        let expected  = eyre!("Still implement, must start with v");
        assert!(matches!(res, Err(expected)));
    }

    #[test]
    fn extract_st_link() -> Result<()>
    {
        let res: ProbeIdentity = String::from("Black Magic Probe (ST-Link/v2) v1.10.0-1273-g2b1ce9aee").try_into()?;

        assert_eq!(res.variant(), Probe::Stlink);
        assert_eq!(res.version, Version::Known(String::from("v1.10.0-1273-g2b1ce9aee")));
        Ok(())
    }

    #[test]
    fn extract_without_closing()
    {
        let res: Result<ProbeIdentity> = "Black Magic Probe (ST-Link".to_string().try_into();

        let expected  = eyre!("Error while parsing probe string: Not a matching pair of parenthesis found.");
        assert!(matches!(res, Err(expected)));
    }

    #[test]
    fn unknown()
    {
        let res: Result<ProbeIdentity> = String::from("Something (v1.2.3)").try_into();

        let expected  = eyre!("Product string doesn't start with 'Black Magic Probe'");
        assert!(matches!(res, Err(expected)));
    }
}