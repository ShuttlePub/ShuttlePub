use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct DisplayName(String);

impl From<String> for DisplayName {
    fn from(prime: String) -> Self {
        Self(prime)
    }
}

impl AsRef<str> for DisplayName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl DisplayName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Summary(String);

impl From<String> for Summary {
    fn from(prime: String) -> Self {
        Self(prime)
    }
}


impl AsRef<str> for Summary {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Summary {
    pub fn new(summary: impl Into<String>) -> Self {
        Self(summary.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Icon(String);

impl From<String> for Icon {
    fn from(prime: String) -> Self {
        Self(prime)
    }
}

impl AsRef<str> for Icon {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Icon {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Banner(String);

impl From<String> for Banner {
    fn from(prime: String) -> Self {
        Self(prime)
    }
}

impl AsRef<str> for Banner {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Banner {
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Profile {
    name: DisplayName,
    summary: Summary,

    icon: Icon,
    banner: Banner
}

impl Profile {
    pub fn new(
        name: impl Into<String>,
        summary: impl Into<String>,
        icon: impl Into<String>,
        banner: impl Into<String>
    ) -> Self {
        Self {
            name: DisplayName::new(name),
            summary: Summary::new(summary),
            icon: Icon::new(icon),
            banner: Banner::new(banner)
        }
    }
}

#[cfg(test)]
mod test {
    use crate::entities::Profile;

    #[test]
    fn struct_test() {
        let _profile = Profile::new("Shuttle", "This is Shuttle!", "example.com", "example.com");
    }
}