use std::collections::HashMap;

use nextjson::{NsonDeserialize, NsonSerialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, NsonSerialize, NsonDeserialize)]
#[njson(rename_all = "lowercase")]
pub enum Vendor {
    Cloudflare,
    Aliyun,
    Tencent,
}

impl Vendor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cloudflare => "cloudflare",
            Self::Aliyun => "aliyun",
            Self::Tencent => "tencent",
        }
    }

    pub fn from_index(i: i32) -> Option<Self> {
        match i {
            0 => Some(Self::Aliyun),
            1 => Some(Self::Tencent),
            2 => Some(Self::Cloudflare),
            _ => None,
        }
    }

    pub fn index(self) -> i32 {
        match self {
            Self::Aliyun => 0,
            Self::Tencent => 1,
            Self::Cloudflare => 2,
        }
    }
}

#[derive(Debug, Clone, NsonSerialize, NsonDeserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub vendor: Vendor,
    #[njson(default, rename = "accessKeyId")]
    pub access_key_id: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize)]
pub struct ProviderFile {
    #[njson(default)]
    pub providers: Vec<Provider>,
}

#[derive(Debug, Clone, NsonSerialize, NsonDeserialize)]
pub struct Cert {
    pub id: String,
    pub domains: Vec<String>,
    #[njson(rename = "providerId")]
    pub provider_id: String,
    #[njson(rename = "notAfter")]
    pub not_after: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize)]
pub struct CertFile {
    #[njson(default)]
    pub certs: Vec<Cert>,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize)]
pub struct SecretsFile {
    #[njson(default)]
    pub secrets: HashMap<String, String>,
}
