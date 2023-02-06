use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct DisplayName(String);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Summary(String);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Icon(String);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Banner(String);

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Profile {
    name: DisplayName,
    summary: Summary,

    icon: Icon,
    banner: Banner
}