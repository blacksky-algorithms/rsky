//! Deployment branding for the browser UI, read from the same environment
//! variables the reference PDS uses so a configuration written for it
//! carries over unchanged.

use rsky_common::env::env_str;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandingError(pub String);

impl fmt::Display for BrandingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BrandingError {}

impl RgbColor {
    /// `#rgb`, `#rrggbb`, or `rgb(r, g, b)`. Alpha is rejected like the
    /// reference does, since the channels feed `rgb(var(--x))` in CSS.
    pub fn parse(input: &str) -> Result<Self, BrandingError> {
        let value = input.trim();
        if let Some(hex) = value.strip_prefix('#') {
            return match hex.len() {
                3 => {
                    let digit = |i: usize| {
                        u8::from_str_radix(&hex[i..=i], 16)
                            .map(|d| d * 17)
                            .map_err(|_| BrandingError(format!("invalid color `{input}`")))
                    };
                    Ok(RgbColor {
                        r: digit(0)?,
                        g: digit(1)?,
                        b: digit(2)?,
                    })
                }
                6 => {
                    let channel = |i: usize| {
                        u8::from_str_radix(&hex[i..i + 2], 16)
                            .map_err(|_| BrandingError(format!("invalid color `{input}`")))
                    };
                    Ok(RgbColor {
                        r: channel(0)?,
                        g: channel(2)?,
                        b: channel(4)?,
                    })
                }
                4 | 8 => Err(BrandingError(format!(
                    "alpha values are not supported in `{input}`"
                ))),
                _ => Err(BrandingError(format!("invalid color `{input}`"))),
            };
        }
        if let Some(inner) = value
            .strip_prefix("rgb(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let channels: Vec<&str> = inner.split(',').map(str::trim).collect();
            if channels.len() != 3 {
                return Err(BrandingError(format!("invalid color `{input}`")));
            }
            let channel = |s: &str| {
                s.parse::<u8>()
                    .map_err(|_| BrandingError(format!("invalid color `{input}`")))
            };
            return Ok(RgbColor {
                r: channel(channels[0])?,
                g: channel(channels[1])?,
                b: channel(channels[2])?,
            });
        }
        if value.starts_with("rgba(") {
            return Err(BrandingError(format!(
                "alpha values are not supported in `{input}`"
            )));
        }
        Err(BrandingError(format!("invalid color `{input}`")))
    }

    /// Space-separated channels, the form the stylesheet's `rgb(var(--x))`
    /// tokens expect.
    pub fn channels(&self) -> String {
        format!("{} {} {}", self.r, self.g, self.b)
    }

    fn luminance(&self) -> f64 {
        fn linear(c: u8) -> f64 {
            let v = f64::from(c) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }
}

/// Black or white, whichever reads better on `color` by WCAG 2.1 contrast;
/// white wins ties, matching the reference fallback.
pub fn contrast_foreground(color: &RgbColor) -> RgbColor {
    let l = color.luminance();
    let white = 1.05 / (l + 0.05);
    let black = (l + 0.05) / 0.05;
    if black > white {
        RgbColor { r: 0, g: 0, b: 0 }
    } else {
        RgbColor {
            r: 255,
            g: 255,
            b: 255,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrandLink {
    pub title: &'static str,
    pub href: String,
    pub rel: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Branding {
    pub service_name: String,
    pub logo_url: Option<String>,
    pub primary: Option<RgbColor>,
    pub error: Option<RgbColor>,
    pub warning: Option<RgbColor>,
    pub info: Option<RgbColor>,
    pub success: Option<RgbColor>,
    pub background_light_url: Option<String>,
    pub background_dark_url: Option<String>,
    pub home_url: Option<String>,
    pub terms_of_service_url: Option<String>,
    pub privacy_policy_url: Option<String>,
    pub support_url: Option<String>,
    /// The client app this deployment is built around, named in copy that
    /// would otherwise have to name one
    pub app_name: Option<String>,
    pub app_url: Option<String>,
    /// The operator, for the About page
    pub org_name: Option<String>,
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Branding {
    /// Reads the reference variable names; an unparseable color is a
    /// configuration error and stops startup like other bad settings do.
    pub fn from_env(hostname: &str) -> Result<Self, BrandingError> {
        let color = |name: &str| -> Result<Option<RgbColor>, BrandingError> {
            match non_empty(env_str(name)) {
                Some(value) => RgbColor::parse(&value)
                    .map(Some)
                    .map_err(|e| BrandingError(format!("{name}: {e}"))),
                None => Ok(None),
            }
        };
        Ok(Branding {
            service_name: non_empty(env_str("PDS_SERVICE_NAME"))
                .unwrap_or_else(|| format!("{hostname} PDS")),
            logo_url: non_empty(env_str("PDS_LOGO_URL")),
            primary: color("PDS_PRIMARY_COLOR")?,
            error: color("PDS_ERROR_COLOR")?,
            warning: color("PDS_WARNING_COLOR")?,
            info: color("PDS_INFO_COLOR")?,
            success: color("PDS_SUCCESS_COLOR")?,
            background_light_url: non_empty(env_str("PDS_BACKGROUND_LIGHT_URL")),
            background_dark_url: non_empty(env_str("PDS_BACKGROUND_DARK_URL")),
            home_url: non_empty(env_str("PDS_HOME_URL")),
            terms_of_service_url: non_empty(env_str("PDS_TERMS_OF_SERVICE_URL")),
            privacy_policy_url: non_empty(env_str("PDS_PRIVACY_POLICY_URL")),
            support_url: non_empty(env_str("PDS_SUPPORT_URL")),
            app_name: non_empty(env_str("PDS_APP_NAME")),
            app_url: non_empty(env_str("PDS_APP_URL")),
            org_name: non_empty(env_str("PDS_ORG_NAME")),
        })
    }

    /// The `--branding-*` variables the stylesheet resolves; only configured
    /// values are emitted so every other token keeps its stock fallback.
    pub fn css_vars(&self) -> String {
        let mut vars: Vec<String> = Vec::new();
        if let Some(primary) = &self.primary {
            vars.push(format!("--branding-color-primary: {};", primary.channels()));
            vars.push(format!(
                "--branding-color-primary-contrast: {};",
                contrast_foreground(primary).channels()
            ));
        }
        for (name, value) in [
            ("error", &self.error),
            ("warning", &self.warning),
            ("info", &self.info),
            ("success", &self.success),
        ] {
            if let Some(color) = value {
                vars.push(format!("--branding-color-{name}: {};", color.channels()));
            }
        }
        for (name, value) in [
            ("light", &self.background_light_url),
            ("dark", &self.background_dark_url),
        ] {
            if let Some(url) = value {
                vars.push(format!(
                    "--branding-background-{name}-image: url(\"{}\");",
                    css_string(url)
                ));
            }
        }
        if vars.is_empty() {
            return String::new();
        }
        format!(":root {{ {} }}", vars.join(" "))
    }

    /// Footer links in the reference order; unset ones are simply absent.
    pub fn links(&self) -> Vec<BrandLink> {
        [
            ("Home", &self.home_url, "canonical"),
            (
                "Terms of Service",
                &self.terms_of_service_url,
                "terms-of-service",
            ),
            ("Privacy Policy", &self.privacy_policy_url, "privacy-policy"),
            ("Support", &self.support_url, "help"),
        ]
        .into_iter()
        .filter_map(|(title, href, rel)| {
            href.as_ref().map(|href| BrandLink {
                title,
                href: href.clone(),
                rel,
            })
        })
        .collect()
    }
}

fn css_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rgb_forms() {
        assert_eq!(
            RgbColor::parse("#fff").unwrap(),
            RgbColor {
                r: 255,
                g: 255,
                b: 255
            }
        );
        assert_eq!(
            RgbColor::parse("#8338ec").unwrap(),
            RgbColor {
                r: 131,
                g: 56,
                b: 236
            }
        );
        assert_eq!(
            RgbColor::parse(" #6060E9 ").unwrap(),
            RgbColor {
                r: 96,
                g: 96,
                b: 233
            }
        );
        assert_eq!(
            RgbColor::parse("rgb(131, 56, 236)").unwrap(),
            RgbColor {
                r: 131,
                g: 56,
                b: 236
            }
        );
        assert_eq!(RgbColor::parse("rgb(1,2,3)").unwrap().channels(), "1 2 3");
    }

    #[test]
    fn rejects_alpha_and_garbage() {
        assert!(RgbColor::parse("#8338ecff")
            .unwrap_err()
            .0
            .contains("alpha"));
        assert!(RgbColor::parse("#abcd").unwrap_err().0.contains("alpha"));
        assert!(RgbColor::parse("rgba(1, 2, 3, 0.5)")
            .unwrap_err()
            .0
            .contains("alpha"));
        assert!(RgbColor::parse("purple").is_err());
        assert!(RgbColor::parse("#12345").is_err());
        assert!(RgbColor::parse("#gg0000").is_err());
        assert!(RgbColor::parse("#ggg").is_err());
        assert!(RgbColor::parse("rgb(1, 2)").is_err());
        assert!(RgbColor::parse("rgb(1, 2, 300)").is_err());
        assert_eq!(
            RgbColor::parse("x").unwrap_err().to_string(),
            "invalid color `x`"
        );
    }

    #[test]
    fn picks_the_readable_foreground() {
        let purple = RgbColor::parse("#8338EC").unwrap();
        assert_eq!(contrast_foreground(&purple).channels(), "255 255 255");
        let gold = RgbColor::parse("#FFD700").unwrap();
        assert_eq!(contrast_foreground(&gold).channels(), "0 0 0");
        let white = RgbColor::parse("#ffffff").unwrap();
        assert_eq!(contrast_foreground(&white).channels(), "0 0 0");
    }

    fn sample() -> Branding {
        Branding {
            service_name: "Example".into(),
            primary: Some(RgbColor::parse("#6060E9").unwrap()),
            error: Some(RgbColor { r: 1, g: 2, b: 3 }),
            background_light_url: Some("https://x.test/a\"b\\c.png".into()),
            home_url: Some("https://home.test".into()),
            support_url: Some("https://help.test".into()),
            ..Branding::default()
        }
    }

    #[test]
    fn emits_only_configured_css_variables() {
        assert_eq!(
            sample().css_vars(),
            ":root { --branding-color-primary: 96 96 233; --branding-color-primary-contrast: 255 255 255; --branding-color-error: 1 2 3; --branding-background-light-image: url(\"https://x.test/a\\\"b\\\\c.png\"); }"
        );
        assert_eq!(Branding::default().css_vars(), "");
        let all = Branding {
            warning: Some(RgbColor { r: 4, g: 5, b: 6 }),
            info: Some(RgbColor { r: 7, g: 8, b: 9 }),
            success: Some(RgbColor {
                r: 10,
                g: 11,
                b: 12,
            }),
            background_dark_url: Some("https://x.test/d.png".into()),
            ..Branding::default()
        };
        let css = all.css_vars();
        assert!(css.contains("--branding-color-warning: 4 5 6;"));
        assert!(css.contains("--branding-color-info: 7 8 9;"));
        assert!(css.contains("--branding-color-success: 10 11 12;"));
        assert!(css.contains("--branding-background-dark-image: url(\"https://x.test/d.png\");"));
        assert!(!css.contains("primary"));
    }

    #[test]
    fn links_follow_the_reference_order_and_skip_unset() {
        let links = sample().links();
        assert_eq!(
            links.iter().map(|l| (l.title, l.rel)).collect::<Vec<_>>(),
            vec![("Home", "canonical"), ("Support", "help")]
        );
        assert_eq!(links[0].href, "https://home.test");
        assert!(Branding::default().links().is_empty());
    }

    #[test]
    fn reads_the_environment_with_reference_defaults() {
        let vars = [
            "PDS_SERVICE_NAME",
            "PDS_LOGO_URL",
            "PDS_PRIMARY_COLOR",
            "PDS_ERROR_COLOR",
            "PDS_WARNING_COLOR",
            "PDS_INFO_COLOR",
            "PDS_SUCCESS_COLOR",
            "PDS_BACKGROUND_LIGHT_URL",
            "PDS_BACKGROUND_DARK_URL",
            "PDS_HOME_URL",
            "PDS_TERMS_OF_SERVICE_URL",
            "PDS_PRIVACY_POLICY_URL",
            "PDS_SUPPORT_URL",
            "PDS_APP_NAME",
            "PDS_APP_URL",
            "PDS_ORG_NAME",
        ];
        // one variable is always set going in, so the restore below puts a
        // value back as well as clearing the rest
        std::env::set_var("PDS_ORG_NAME", "Kept");
        let saved: Vec<(&str, Option<String>)> =
            vars.iter().map(|v| (*v, std::env::var(v).ok())).collect();
        for v in vars {
            std::env::remove_var(v);
        }
        let branding = Branding::from_env("pds.test").unwrap();
        assert_eq!(branding.service_name, "pds.test PDS");
        assert_eq!(branding.primary, None);
        assert!(branding.links().is_empty());

        std::env::set_var("PDS_SERVICE_NAME", " Example ");
        std::env::set_var("PDS_PRIMARY_COLOR", "#6060E9");
        std::env::set_var("PDS_LOGO_URL", "");
        std::env::set_var("PDS_APP_NAME", "App");
        std::env::set_var("PDS_TERMS_OF_SERVICE_URL", "https://tos.test");
        let branding = Branding::from_env("pds.test").unwrap();
        assert_eq!(branding.service_name, "Example");
        assert_eq!(branding.primary.unwrap().channels(), "96 96 233");
        assert_eq!(branding.logo_url, None);
        assert_eq!(branding.app_name.as_deref(), Some("App"));
        assert_eq!(branding.links()[0].rel, "terms-of-service");

        std::env::set_var("PDS_ERROR_COLOR", "#12345678");
        let err = Branding::from_env("pds.test").unwrap_err();
        assert!(err.0.starts_with("PDS_ERROR_COLOR:"), "{err}");

        for (v, value) in saved {
            match value {
                Some(value) => std::env::set_var(v, value),
                None => std::env::remove_var(v),
            }
        }
        assert_eq!(std::env::var("PDS_ORG_NAME").as_deref(), Ok("Kept"));
        std::env::remove_var("PDS_ORG_NAME");
    }
}
