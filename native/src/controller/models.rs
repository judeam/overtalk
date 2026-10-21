use super::*;

impl Controller {
    pub(super) fn choose_provider(&mut self, ui: &AppWindow) {
        let speech = ui.global::<State>().get_settings_tab() == 2;
        let mut providers = if speech {
            vec![
                ("localWhisper", "Whisper · on this device"),
                ("parakeet", "Parakeet · on this device"),
                ("localSpeech", "Advanced local speech"),
                ("openai", "OpenAI"),
                ("groq", "Groq"),
                ("deepgram", "Deepgram"),
                ("elevenLabs", "ElevenLabs"),
            ]
        } else {
            vec![
                ("builtin-ai", "Built-in AI · on this device"),
                ("ollama", "Ollama"),
                ("openai", "OpenAI"),
                ("claude", "Anthropic"),
                ("groq", "Groq"),
                ("openrouter", "OpenRouter"),
                ("privatemode", "Privatemode · encrypted cloud AI"),
                ("custom-openai", "OpenAI-compatible provider"),
            ]
        };
        if !speech && ui.global::<State>().get_cloud_available() {
            providers.insert(0, ("overtalk-cloud", "Private AI summaries"));
        }
        self.choices(
            ui,
            "provider",
            "Choose a provider",
            providers
                .into_iter()
                .map(|(id, label)| (id.into(), label.into()))
                .collect(),
            Value::Null,
        );
    }
    pub(super) fn show_models(&mut self, ui: &AppWindow) {
        let provider = ui.global::<State>().get_provider();
        match provider.as_str() {
            "overtalk-cloud" => self.choices(
                ui,
                "models",
                "Private AI summaries",
                vec![("select:glm-5.3-flash".into(), "GLM-5.3-Flash".into())],
                Value::String(provider.to_string()),
            ),
            "localWhisper" => self.request(ui, "whisper_init", json!({}), "models-init", &provider),
            "parakeet" => self.request(ui, "parakeet_init", json!({}), "models-init", &provider),
            "builtin-ai" => {
                self.request(ui, "builtin_ai_list_models", json!({}), "models", &provider)
            }
            "ollama" => self.request(
                ui,
                "get_ollama_models",
                json!({"endpoint":ui.global::<State>().get_endpoint().as_str()}),
                "models",
                &provider,
            ),
            "privatemode" => self.request(
                ui,
                "api_get_privatemode_models",
                json!({"endpoint":ui.global::<State>().get_endpoint().as_str(),
                       "apiKey":ui.global::<State>().get_api_key().as_str()}),
                "models",
                &provider,
            ),
            _ => ui
                .global::<State>()
                .set_message("Enter the model ID supplied by your provider, then save.".into()),
        }
    }
    pub(super) fn model_list(&self, ui: &AppWindow, provider: &str) {
        self.request(
            ui,
            if provider == "parakeet" {
                "parakeet_get_available_models"
            } else {
                "whisper_get_available_models"
            },
            json!({}),
            "models",
            provider,
        );
    }
    pub(super) fn show_model_rows(&mut self, ui: &AppWindow, provider: &str, value: Value) {
        let rows = array(&value)
            .iter()
            .flat_map(|m| {
                let name = text(m, "name");
                let available = provider == "ollama"
                    || m["status"] == "Available"
                    || m["status"]["type"] == "available";
                let downloading = m["status"].get("Downloading").is_some()
                    || m["status"]["type"] == "downloading";
                let action = if available {
                    "select"
                } else if downloading {
                    "cancel"
                } else {
                    "download"
                };
                let label = if text(m, "display_name").is_empty() {
                    name.clone()
                } else {
                    text(m, "display_name")
                };
                let mut choices = vec![(
                    format!("{action}:{name}"),
                    format!(
                        "{} · {}{}",
                        if text(m, "display_name").is_empty() {
                            name.clone()
                        } else {
                            text(m, "display_name")
                        },
                        if available {
                            "Use this model"
                        } else if downloading {
                            "Cancel download"
                        } else {
                            "Download"
                        },
                        m["size_mb"]
                            .as_u64()
                            .map(|s| format!(" · {s} MB"))
                            .unwrap_or_default()
                    ),
                )];
                if provider != "ollama"
                    && provider != "privatemode"
                    && (available || m["status"]["type"] == "corrupted")
                {
                    choices.push((
                        format!("delete:{name}"),
                        format!("Remove {label} from this device…"),
                    ));
                }
                choices
            })
            .collect();
        self.choices(ui, "models", "Models", rows, Value::String(provider.into()));
    }
    pub(super) fn model_choice(&mut self, ui: &AppWindow, provider: &str, value: &str) {
        let Some((action, name)) = value.split_once(':') else {
            return;
        };
        if action == "select" {
            ui.global::<State>().set_model(name.into());
            ui.global::<State>()
                .set_message("Model selected. Save provider to apply.".into());
            return;
        }
        if action == "delete" {
            self.dialog(
                ui,
                "delete-model",
                &format!("Remove {name}?"),
                "You can download it again. Meeting recordings and notes are preserved.",
                "confirm",
                json!({"provider":provider,"name":name}),
            );
            ui.global::<State>().set_modal_submit("Remove model".into());
            return;
        }
        // Keep cancellation reachable while the asynchronous download runs.
        if action == "download" {
            let state = ui.global::<State>();
            let rows: Vec<DetailRow> = state
                .get_modal_choices()
                .iter()
                .map(|mut row| {
                    if row.id == value {
                        row.id = format!("cancel:{name}").into();
                        row.title = format!("Cancel download of {name}").into();
                    }
                    row
                })
                .collect();
            // Keep the unfiltered catalogue when a download starts from a search.
            for cached in &mut self.choice_rows {
                if cached.id == value {
                    cached.id = format!("cancel:{name}").into();
                    cached.title = format!("Cancel download of {name}").into();
                }
            }
            state.set_modal_choices(model(rows));
            state.set_modal("choice".into());
        }
        let command = match (provider, action) {
            ("localWhisper", "download") => "whisper_download_model",
            ("localWhisper", "cancel") => "whisper_cancel_download",
            ("parakeet", "download") => "parakeet_download_model",
            ("parakeet", "cancel") => "parakeet_cancel_download",
            ("builtin-ai", "download") => "builtin_ai_download_model",
            ("builtin-ai", "cancel") => "builtin_ai_cancel_download",
            _ => return,
        };
        self.request(
            ui,
            command,
            json!({"modelName":name}),
            "model-changed",
            provider,
        );
        ui.global::<State>().set_message(
            if action == "download" {
                "Downloading model…"
            } else {
                "Cancelling download…"
            }
            .into(),
        );
    }
}
