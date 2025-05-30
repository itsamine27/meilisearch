use actix_web::web::{self, Data};
use actix_web::HttpResponse;
use deserr::Deserr;
use index_scheduler::IndexScheduler;
use meilisearch_types::deserr::DeserrJsonError;
use meilisearch_types::error::deserr_codes::*;
use meilisearch_types::error::{Code, ResponseError};
use meilisearch_types::features::{
    ChatCompletionPrompts as DbChatCompletionPrompts, ChatCompletionSettings,
    ChatCompletionSource as DbChatCompletionSource, DEFAULT_CHAT_PRE_QUERY_PROMPT,
    DEFAULT_CHAT_SEARCH_DESCRIPTION_PROMPT, DEFAULT_CHAT_SEARCH_INDEX_UID_PARAM_PROMPT,
    DEFAULT_CHAT_SEARCH_Q_PARAM_PROMPT, DEFAULT_CHAT_SYSTEM_PROMPT,
};
use meilisearch_types::keys::actions;
use meilisearch_types::milli::update::Setting;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::extractors::authentication::policies::ActionPolicy;
use crate::extractors::authentication::GuardedData;
use crate::extractors::sequential_extractor::SeqHandler;

use super::ChatsParam;

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("")
            .route(web::get().to(SeqHandler(get_settings)))
            .route(web::patch().to(SeqHandler(patch_settings)))
            .route(web::delete().to(SeqHandler(delete_settings))),
    );
}

async fn get_settings(
    index_scheduler: GuardedData<
        ActionPolicy<{ actions::CHATS_SETTINGS_GET }>,
        Data<IndexScheduler>,
    >,
    chats_param: web::Path<ChatsParam>,
) -> Result<HttpResponse, ResponseError> {
    let ChatsParam { workspace_uid } = chats_param.into_inner();

    // TODO do a spawn_blocking here ???
    let rtxn = index_scheduler.read_txn()?;
    let mut settings = match index_scheduler.chat_settings(&rtxn, &workspace_uid)? {
        Some(settings) => settings,
        None => {
            return Err(ResponseError::from_msg(
                format!("Chat `{workspace_uid}` not found"),
                Code::ChatWorkspaceNotFound,
            ))
        }
    };
    settings.hide_secrets();
    Ok(HttpResponse::Ok().json(settings))
}

async fn patch_settings(
    index_scheduler: GuardedData<
        ActionPolicy<{ actions::CHATS_SETTINGS_UPDATE }>,
        Data<IndexScheduler>,
    >,
    chats_param: web::Path<ChatsParam>,
    web::Json(new): web::Json<GlobalChatSettings>,
) -> Result<HttpResponse, ResponseError> {
    let ChatsParam { workspace_uid } = chats_param.into_inner();

    // TODO do a spawn_blocking here
    let mut wtxn = index_scheduler.write_txn()?;
    let old_settings =
        index_scheduler.chat_settings(&mut wtxn, &workspace_uid)?.unwrap_or_default();

    let prompts = match new.prompts {
        Setting::Set(new_prompts) => DbChatCompletionPrompts {
            system: match new_prompts.system {
                Setting::Set(new_system) => new_system,
                Setting::Reset => DEFAULT_CHAT_SYSTEM_PROMPT.to_string(),
                Setting::NotSet => old_settings.prompts.system,
            },
            search_description: match new_prompts.search_description {
                Setting::Set(new_description) => new_description,
                Setting::Reset => DEFAULT_CHAT_SEARCH_DESCRIPTION_PROMPT.to_string(),
                Setting::NotSet => old_settings.prompts.search_description,
            },
            search_q_param: match new_prompts.search_q_param {
                Setting::Set(new_description) => new_description,
                Setting::Reset => DEFAULT_CHAT_SEARCH_Q_PARAM_PROMPT.to_string(),
                Setting::NotSet => old_settings.prompts.search_q_param,
            },
            search_index_uid_param: match new_prompts.search_index_uid_param {
                Setting::Set(new_description) => new_description,
                Setting::Reset => DEFAULT_CHAT_SEARCH_INDEX_UID_PARAM_PROMPT.to_string(),
                Setting::NotSet => old_settings.prompts.search_index_uid_param,
            },
            pre_query: match new_prompts.pre_query {
                Setting::Set(new_description) => new_description,
                Setting::Reset => DEFAULT_CHAT_PRE_QUERY_PROMPT.to_string(),
                Setting::NotSet => old_settings.prompts.pre_query,
            },
        },
        Setting::Reset => DbChatCompletionPrompts::default(),
        Setting::NotSet => old_settings.prompts,
    };

    let settings = ChatCompletionSettings {
        source: match new.source {
            Setting::Set(new_source) => new_source.into(),
            Setting::Reset => DbChatCompletionSource::default(),
            Setting::NotSet => old_settings.source,
        },
        base_api: match new.base_api {
            Setting::Set(new_base_api) => Some(new_base_api),
            Setting::Reset => None,
            Setting::NotSet => old_settings.base_api,
        },
        api_key: match new.api_key {
            Setting::Set(new_api_key) => Some(new_api_key),
            Setting::Reset => None,
            Setting::NotSet => old_settings.api_key,
        },
        prompts,
    };

    // TODO send analytics
    // analytics.publish(
    //     PatchNetworkAnalytics {
    //         network_size: merged_remotes.len(),
    //         network_has_self: merged_self.is_some(),
    //     },
    //     &req,
    // );

    index_scheduler.put_chat_settings(&mut wtxn, &workspace_uid, &settings)?;
    wtxn.commit()?;

    Ok(HttpResponse::Ok().json(settings))
}

async fn delete_settings(
    index_scheduler: GuardedData<
        ActionPolicy<{ actions::CHATS_SETTINGS_DELETE }>,
        Data<IndexScheduler>,
    >,
    chats_param: web::Path<ChatsParam>,
) -> Result<HttpResponse, ResponseError> {
    let ChatsParam { workspace_uid } = chats_param.into_inner();

    // TODO do a spawn_blocking here
    let mut wtxn = index_scheduler.write_txn()?;
    if index_scheduler.delete_chat_settings(&mut wtxn, &workspace_uid)? {
        wtxn.commit()?;
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ResponseError::from_msg(
            format!("Chat `{workspace_uid}` not found"),
            Code::ChatWorkspaceNotFound,
        ))
    }
}

#[derive(Debug, Clone, Deserialize, Deserr, ToSchema)]
#[deserr(error = DeserrJsonError, rename_all = camelCase, deny_unknown_fields)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schema(rename_all = "camelCase")]
pub struct GlobalChatSettings {
    #[serde(default)]
    #[deserr(default)]
    #[schema(value_type = Option<ChatCompletionSource>)]
    pub source: Setting<ChatCompletionSource>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionBaseApi>)]
    #[schema(value_type = Option<String>, example = json!("https://api.mistral.ai/v1"))]
    pub base_api: Setting<String>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionApiKey>)]
    #[schema(value_type = Option<String>, example = json!("abcd1234..."))]
    pub api_key: Setting<String>,
    #[serde(default)]
    #[deserr(default)]
    #[schema(inline, value_type = Option<ChatPrompts>)]
    pub prompts: Setting<ChatPrompts>,
}

#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize, Deserr, ToSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[deserr(error = DeserrJsonError, rename_all = camelCase, deny_unknown_fields)]
pub enum ChatCompletionSource {
    #[default]
    OpenAi,
}

impl From<ChatCompletionSource> for DbChatCompletionSource {
    fn from(source: ChatCompletionSource) -> Self {
        match source {
            ChatCompletionSource::OpenAi => DbChatCompletionSource::OpenAi,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Deserr, ToSchema)]
#[deserr(error = DeserrJsonError, rename_all = camelCase, deny_unknown_fields)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schema(rename_all = "camelCase")]
pub struct ChatPrompts {
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionSystemPrompt>)]
    #[schema(value_type = Option<String>, example = json!("You are a helpful assistant..."))]
    pub system: Setting<String>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionSearchDescriptionPrompt>)]
    #[schema(value_type = Option<String>, example = json!("This is the search function..."))]
    pub search_description: Setting<String>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionSearchQueryParamPrompt>)]
    #[schema(value_type = Option<String>, example = json!("This is query parameter..."))]
    pub search_q_param: Setting<String>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionSearchIndexUidParamPrompt>)]
    #[schema(value_type = Option<String>, example = json!("This is index you want to search in..."))]
    pub search_index_uid_param: Setting<String>,
    #[serde(default)]
    #[deserr(default, error = DeserrJsonError<InvalidChatCompletionPreQueryPrompt>)]
    #[schema(value_type = Option<String>)]
    pub pre_query: Setting<String>,
}
