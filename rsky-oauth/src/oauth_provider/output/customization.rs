#[derive(Debug, Clone, PartialEq)]
pub struct CustomizationLink {
    pub href: String,
    pub title: String,
    pub rel: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Customization {
    pub name: Option<String>,
    pub logo: Option<String>,
    pub colors: Option<String>,
    pub links: Option<Vec<CustomizationLink>>,
}
