#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct BusMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<uuid::Uuid>,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_json::Value>,
}

impl BusMetadata {
    pub fn new(
        request_id: Option<uuid::Uuid>,
        correlation_id: Option<uuid::Uuid>,
        causation_id: Option<uuid::Uuid>,
        extra: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Self {
        BusMetadata {
            request_id,
            correlation_id,
            causation_id,
            extra: extra.unwrap_or_default(),
        }
    }

    pub fn add<T: serde::Serialize + serde::de::DeserializeOwned + Clone>(
        &mut self,
        key: &str,
        value: T,
    ) -> Result<(), crate::BusError> {
        let json_value = serde_json::to_value(value).map_err(|e| {
            crate::BusError::MetaData(format!(
                "Failed to serialize metadata value for key '{}': {}",
                key, e
            ))
        })?;

        match key {
            "request_id" | "correlation_id" | "causation_id" => {
                let id: uuid::Uuid = serde_json::from_value(json_value.clone()).map_err(|e| {
                    crate::BusError::MetaData(format!(
                        "Metadata key '{}' expects a valid UUID, but got: {}. Error: {}",
                        key, json_value, e
                    ))
                })?;

                match key {
                    "request_id" => self.request_id = Some(id),
                    "correlation_id" => self.correlation_id = Some(id),
                    "causation_id" => self.causation_id = Some(id),
                    _ => unreachable!(),
                }
            }
            _ => {
                self.extra.insert(key.to_string(), json_value);
            }
        }
        Ok(())
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.extra
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn try_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T, crate::BusError> {
        let value = self.extra.get(key).ok_or_else(|| {
            crate::BusError::MetaData(format!("Metadata key '{}' not found", key))
        })?;
        serde_json::from_value(value.clone()).map_err(|e| {
            crate::BusError::MetaData(format!("Metadata type mismatch for '{}': {}", key, e))
        })
    }
}
