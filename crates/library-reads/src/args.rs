//! Strict arguments: unknown names and wrong types are errors, never ignored.
use serde_json::{Map, Value};
use std::ops::RangeInclusive;

pub(crate) struct Args<'a>(&'a Map<String, Value>);

impl<'a> Args<'a> {
    pub fn new(value: &'a Value, allowed: &[&str]) -> Result<Self, String> {
        let map = value.as_object().ok_or("Arguments must be an object")?;
        if let Some(key) = map.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(format!(
                "Unknown argument “{key}”. Use: {}",
                allowed.join(", ")
            ));
        }
        Ok(Args(map))
    }

    pub fn text(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) if !text.trim().is_empty() && text.len() <= 1000 => {
                Ok(Some(text.trim().to_owned()))
            }
            _ => Err(format!(
                "{key} must be a nonempty string of at most 1000 bytes"
            )),
        }
    }

    pub fn required(&self, key: &str) -> Result<String, String> {
        self.text(key)?.ok_or_else(|| format!("{key} is required"))
    }

    pub fn number(
        &self,
        key: &str,
        default: i64,
        range: RangeInclusive<i64>,
    ) -> Result<i64, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(value) => value
                .as_i64()
                .filter(|number| range.contains(number))
                .ok_or_else(|| {
                    format!(
                        "{key} must be an integer from {} to {}",
                        range.start(),
                        range.end()
                    )
                }),
        }
    }

    pub fn flag(&self, key: &str, default: bool) -> Result<bool, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(format!("{key} must be true or false")),
        }
    }

    pub fn choice(&self, key: &str, choices: &[&str]) -> Result<Option<String>, String> {
        match self.text(key)? {
            Some(value) if choices.contains(&value.as_str()) => Ok(Some(value)),
            None => Ok(None),
            Some(_) => Err(format!("{key} must be one of: {}", choices.join(", "))),
        }
    }

    /// A calendar date written YYYY-MM-DD.
    pub fn date(&self, key: &str) -> Result<Option<String>, String> {
        let Some(value) = self.text(key)? else {
            return Ok(None);
        };
        let parts: Vec<_> = value.split('-').collect();
        let valid = parts.len() == 3
            && [4, 2, 2]
                .iter()
                .zip(&parts)
                .all(|(len, part)| part.len() == *len && part.bytes().all(|b| b.is_ascii_digit()))
            && (1..=12).contains(&parts[1].parse::<u32>().unwrap_or(0))
            && (1..=31).contains(&parts[2].parse::<u32>().unwrap_or(0));
        if valid {
            Ok(Some(value))
        } else {
            Err(format!("{key} must be a date written YYYY-MM-DD"))
        }
    }
}
