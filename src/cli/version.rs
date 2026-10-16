//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmseed version`: the same line as --version, or the parts as JSON.

use anstream::println;

use crate::brand;
use crate::errors::AppResult;
use crate::utilities::ui::Ui;

pub fn run(ui: Ui) -> AppResult<()> {
    if ui.json {
        ui.json(&serde_json::json!({
            "name": brand::NAME,
            "version": brand::VERSION,
            "commit": brand::GIT_SHA,
            "date": brand::BUILD_DATE,
            "target": brand::TARGET,
        }));
    } else {
        //Printed even with --quiet: asking for the version means wanting to see it.
        println!("{} {}", brand::NAME, brand::LONG_VERSION);
    }
    Ok(())
}
