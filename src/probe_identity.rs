use color_eyre::eyre::{eyre, Result};
use color_eyre::Report;
use log::warn;
use crate::metadata::structs::Probe;

const BMP_PRODUCT_STRING: &str = "Black Magic Probe";
const BMP_PRODUCT_STRING_LENGTH: usize = BMP_PRODUCT_STRING.len();

struct ProbeName{
    name: String,
}
struct ProbeNameVersion{
    name: String,
    version: String,
}

pub enum ProbeIdentity{
    ProbeVersion(ProbeNameVersion),
    Probe(ProbeName),
    None
}

const NATIVE_BPM: &str = "native";

fn probe_from_bpm(input: &str) -> Result<String>{
    let opening_paren = input.find('(');
    let closing_paren = input.find(')');

    match (opening_paren, closing_paren) {
        (None, None) => Ok(NATIVE_BPM.into()),
        (Some(opening_paren), Some(closing_paren)) => {
            if opening_paren > closing_paren {
                Err(eyre!("'(' is defined after ')'"))
            }
            else{
               Ok(input[opening_paren+1..closing_paren].to_string().to_lowercase())
            }
        }
        (Some(_), None) => Err(eyre!("No ')' defined")),
        (None, Some(_)) => Err(eyre!("No '(' defined")),
    }
}

fn version_from_bpm(input: &str) -> Result<String>{
    let start_index = input.rfind(' ')
        .ok_or_else(|| eyre!("Invalid version string"))?;

    let version = input[start_index + 1..].to_string();

    if version.trim().is_empty() {
        return Err(eyre!("Version is empty or only whitespace"));
    }

    Ok(version)
}

impl From<String> for ProbeIdentity {
    fn from(identity: String) -> Self {
        if identity == BMP_PRODUCT_STRING {
            ProbeIdentity::Probe(ProbeName {
                name: NATIVE_BPM.into()
            })
        }
        else if identity.starts_with(BMP_PRODUCT_STRING) {
            let probe_result = probe_from_bpm(&identity[BMP_PRODUCT_STRING_LENGTH..]);

            let probe = match probe_result {
                Ok(probe) => probe,
                Err(error) => {
                    warn!("Error while parsing probe string: {}", error);
                    return  ProbeIdentity::None
                }
            };
            
            let version = version_from_bpm(&identity[BMP_PRODUCT_STRING_LENGTH..]);

            match version {
                Ok(version) => ProbeIdentity::ProbeVersion(ProbeNameVersion{
                    name: probe,
                    version
                }),
                Err(error) => {
                    warn!("Error while parsing version string: {}", error);
                    ProbeIdentity::None
                }
            }            
        }
        else {
            ProbeIdentity::None
        }

    }
}

impl ProbeIdentity
{
    fn probe_name(&self) -> Option<String>{
        match &self{
            ProbeIdentity::ProbeVersion(probe) => Some(probe.name.to_string()),
            ProbeIdentity::Probe(probe) => Some(probe.name.to_string()),
            ProbeIdentity::None => None,
        }
    }
    
    pub fn variant(&self) -> Result<Probe, Report>
    {
        let probe_version = self.probe_name()
            .ok_or_else(|| eyre!("No product string discovered."))?;
        
        probe_version.as_str().try_into()
    }


    pub fn version(self) -> Option<String> {
        match self{
            ProbeIdentity::ProbeVersion(probe) => Some(probe.version),
            ProbeIdentity::Probe(_) => Some("v1.6".to_string()),
            ProbeIdentity::None => None,
        }
    }
}

impl TryFrom<&str> for Probe {
    type Error = Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "96b carbon" => Ok(Probe::_96bCarbon),
            "blackpill-f401cc" => Ok(Probe::BlackpillF401CC),
            "blackpill-f401ce" => Ok(Probe::BlackpillF401CE),
            "blackpill-f411ce" => Ok(Probe::BlackpillF411CE),
            "ctxlink" => Ok(Probe::CtxLink),
            "f072-if" => Ok(Probe::F072),
            "f3-if" => Ok(Probe::F3),
            "f4discovery" => Ok(Probe::F4Discovery),
            "hydrabus" => Ok(Probe::HydraBus),
            "launchpad icdi" => Ok(Probe::LaunchpadICDI),
            "native" => Ok(Probe::Native),
            "st-link/v2" => Ok(Probe::Stlink),
            "st-link v3" => Ok(Probe::Stlinkv3),
            "swlink" => Ok(Probe::Swlink),
            _ => Err(eyre!("Probe with unknown product string encountered")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_native(){
        let res: ProbeIdentity = "Black Magic Probe v2.0.0-rc2".to_string().into();

        assert_eq!(res.probe_name(), Some("native".into()));
        assert_eq!(res.version(), Some("v2.0.0-rc2".into()));
    }

    #[test]
    fn extract_old(){
        let res: ProbeIdentity = "Black Magic Probe".to_string().into();
        
        assert_eq!(res.probe_name(), Some("native".into()));
        assert_eq!(res.version(), Some("v1.6".into()));
    }

    #[test]
    fn extract_st_link(){
        let res: ProbeIdentity = "Black Magic Probe (ST-Link v2) v1.10.0-1273-g2b1ce9aee".to_string().into();

        assert_eq!(res.probe_name(), Some("st-link v2".into()));
        assert_eq!(res.version(), Some("v1.10.0-1273-g2b1ce9aee".into()));
    }

    #[test]
    fn extract_without_closing(){
        let res: ProbeIdentity = "Black Magic Probe (ST-Link".to_string().into();

        assert_eq!(res.probe_name(), None);
        assert_eq!(res.version(), None);
    }
    
    #[test]
    fn unknown(){
        let res: ProbeIdentity = "Something (v1.2.3)".to_string().into();

        assert_eq!(res.probe_name(), None);
        assert_eq!(res.version(), None);
    }
}
