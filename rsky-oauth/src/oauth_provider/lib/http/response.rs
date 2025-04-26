// use rocket::http::Status;
// use rocket::Response;
//
// pub fn append_header(res: Response, header: &str, value: &str) {
//     // let mut headers = res.headers();
//     // res.set_header(header, )
// }
//
// pub fn write_redirect(mut res: &Response, url: &str, status: Option<Status>) {
//     let status = match status {
//         None => { Status { code: 302 } }
//         Some(res) => { res }
//     };
//     res.set_status(status);
//     res.set_raw_header("Location", url);
// }
