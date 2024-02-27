use actix_web::{
	get,
	web::{Data, Json},
	Responder, Result,
};
use serde::Serialize;
use std::sync::Arc;

use crate::core::Core;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
	version: String,
	name: String,
	game_id: Option<u64>,
	place_ids: Option<Vec<u64>>,
}

#[get("/details")]
async fn main(core: Data<Arc<Core>>) -> Result<impl Responder> {
	let response = Response {
		version: env!("CARGO_PKG_VERSION").to_string(),
		name: core.name(),
		game_id: core.game_id(),
		place_ids: core.place_ids(),
	};

	Ok(Json(response))
}
