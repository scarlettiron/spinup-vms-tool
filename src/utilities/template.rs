//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Fills {{name}} placeholders in a text template. Plain text replacement keeps
//comments and layout exactly as written, which matters for YAML files people
//read and edit by hand.

//Replaces each {{key}} with its value. Placeholders without a value are left as
//they are, so a mistake shows up in the output instead of silently vanishing.
pub fn render(template: &str, values: &[(&str, String)]) -> String {
    let mut output = template.to_string();
    for (key, value) in values {
        output = output.replace(&format!("{{{{{key}}}}}"), value);
    }
    output
}

//Fills the product placeholders ({{app}}, {{App}}, {{dir}}, {{site}}, {{repo}})
//as well as the ones given.
pub fn render_with_brand(template: &str, values: &[(&str, String)]) -> String {
    let mut all = crate::brand::placeholders();
    all.extend(values.iter().map(|(key, value)| (*key, value.clone())));
    render(template, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_known_placeholders_and_keeps_unknown_ones() {
        let text = render(
            "name: {{name}} / {{missing}}",
            &[("name", "acme".to_string())],
        );
        assert_eq!(text, "name: acme / {{missing}}");
    }

    #[test]
    fn brand_placeholders_are_filled() {
        let text = render_with_brand("run {{app}} in {{dir}}", &[]);
        assert_eq!(
            text,
            format!("run {} in {}", crate::brand::NAME, crate::brand::DIR)
        );
    }
}
