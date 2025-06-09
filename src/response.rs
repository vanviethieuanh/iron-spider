use reqwest::StatusCode;

use crate::request::Request;

pub struct Response {
    pub status: StatusCode,
    pub body: String,
    pub request: Request,
}
