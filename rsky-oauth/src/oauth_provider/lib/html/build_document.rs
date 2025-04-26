use rocket::response::content;

const DEFAULT_VIEWPORT: &str = r#"<meta
  name="viewport"
  content="width=device-width, initial-scale=1.0"
/>"#;

pub fn build_document(title: &str, base_href: &str) {
    let html_attrs = attrs_to_html();
    let body_attrs = body_attrs_to_html();
    let body = "";
    let scripts = "";
    let html = format!(
        "<!doctype html>
        <html{html_attrs}>
            <head>
            <meta charset=\"UTF8\" />
            <title>{title}</title>
            <base href=\"{base_href}\" />
            {DEFAULT_VIEWPORT}
            </head>
            <body{body_attrs}>
            {body} {scripts}
            </body>
        </html>"
    );
}

fn is_viewport_meta() {
    unimplemented!()
}

fn link_to_html() {
    unimplemented!()
}

fn meta_to_html() {
    unimplemented!()
}

fn body_attrs_to_html() -> String {
    " <div id=\"root\"></div>".to_string()
}
fn attrs_to_html() -> String {
    " lang=en".to_string()
}

fn script_to_html() {
    unimplemented!()
}

fn style_to_html() {
    unimplemented!()
}
