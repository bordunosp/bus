use crate::contracts::enums::{Field, Period, Replace, State};

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Unique {
    pub period: Period,
    pub fields: &'static [Field],
    pub keys: &'static [&'static str],
    pub states: &'static [State],
    pub replace: &'static [Replace],
}
