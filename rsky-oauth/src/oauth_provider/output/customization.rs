pub struct CustomizationLink {
    pub href: String,
    pub title: String,
    pub rel: Option<String>,
}

pub struct Customization {
    pub name: Option<String>,
    pub logo: Option<String>,
    pub colors: Option<String>,
    pub links: Option<Vec<CustomizationLink>>,
}
