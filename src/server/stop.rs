use actix_web::{post, HttpResponse, Responder};
use log::debug;
use std::process;

use crate::util;

async fn stop() {
	debug!("Stopping Argon!");
	util::kill_process(process::id());
}

#[post("/stop")]
async fn main() -> impl Responder {
	tokio::spawn(stop());
	HttpResponse::Ok().body("Argon stopped")
}
