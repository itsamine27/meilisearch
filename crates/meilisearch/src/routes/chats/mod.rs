use actix_web::web::{self, Data};
use actix_web::HttpResponse;
use deserr::actix_web::AwebQueryParameter;
use deserr::Deserr;
use index_scheduler::IndexScheduler;
use meilisearch_types::deserr::query_params::Param;
use meilisearch_types::deserr::DeserrQueryParamError;
use meilisearch_types::error::deserr_codes::{InvalidIndexLimit, InvalidIndexOffset};
use meilisearch_types::error::ResponseError;
use meilisearch_types::keys::actions;
use serde::{Deserialize, Serialize};
use tracing::debug;
use utoipa::{IntoParams, ToSchema};

use super::Pagination;
use crate::extractors::authentication::policies::ActionPolicy;
use crate::extractors::authentication::GuardedData;
use crate::routes::PAGINATION_DEFAULT_LIMIT;

pub mod chat_completions;
mod errors;
pub mod settings;
mod utils;

/// The function name to report search progress.
const MEILI_SEARCH_PROGRESS_NAME: &str = "_meiliSearchProgress";
/// The function name to append a conversation message in the user conversation.
const MEILI_APPEND_CONVERSATION_MESSAGE_NAME: &str = "_meiliAppendConversationMessage";
/// The function name to report sources to the frontend.
const MEILI_SEARCH_SOURCES_NAME: &str = "_meiliSearchSources";
/// The *internal* function name to provide to the LLM to search in indexes.
const MEILI_SEARCH_IN_INDEX_FUNCTION_NAME: &str = "_meiliSearchInIndex";

#[derive(Deserialize)]
pub struct ChatsParam {
    workspace_uid: String,
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("").route(web::get().to(list_workspaces))).service(
        web::scope("/{workspace_uid}")
            .service(web::scope("/chat/completions").configure(chat_completions::configure))
            .service(web::scope("/settings").configure(settings::configure)),
    );
}

#[derive(Deserr, Debug, Clone, Copy, IntoParams)]
#[deserr(error = DeserrQueryParamError, rename_all = camelCase, deny_unknown_fields)]
#[into_params(rename_all = "camelCase", parameter_in = Query)]
pub struct ListChats {
    /// The number of chat workspaces to skip before starting to retrieve anything
    #[param(value_type = Option<usize>, default, example = 100)]
    #[deserr(default, error = DeserrQueryParamError<InvalidIndexOffset>)]
    pub offset: Param<usize>,
    /// The number of chat workspaces to retrieve
    #[param(value_type = Option<usize>, default = 20, example = 1)]
    #[deserr(default = Param(PAGINATION_DEFAULT_LIMIT), error = DeserrQueryParamError<InvalidIndexLimit>)]
    pub limit: Param<usize>,
}

impl ListChats {
    fn as_pagination(self) -> Pagination {
        Pagination { offset: self.offset.0, limit: self.limit.0 }
    }
}

#[derive(Debug, Serialize, Clone, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChatWorkspaceView {
    /// Unique identifier for the index
    pub uid: String,
}

pub async fn list_workspaces(
    index_scheduler: GuardedData<ActionPolicy<{ actions::CHATS_GET }>, Data<IndexScheduler>>,
    paginate: AwebQueryParameter<ListChats, DeserrQueryParamError>,
) -> Result<HttpResponse, ResponseError> {
    index_scheduler.features().check_chat_completions("Using the /chats settings route")?;

    debug!(parameters = ?paginate, "List chat workspaces");
    let filters = index_scheduler.filters();
    let (total, workspaces) = index_scheduler.paginated_chat_workspace_uids(
        filters,
        *paginate.offset,
        *paginate.limit,
    )?;
    let workspaces =
        workspaces.into_iter().map(|uid| ChatWorkspaceView { uid }).collect::<Vec<_>>();
    let ret = paginate.as_pagination().format_with(total, workspaces);

    debug!(returns = ?ret, "List chat workspaces");
    Ok(HttpResponse::Ok().json(ret))
}
