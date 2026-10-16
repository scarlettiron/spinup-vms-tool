//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//`vmynt blueprints`: lists the blueprints visible from here, and
//`vmynt blueprints new` copies one into the project to customize.

use anstream::print;

use super::args::{BlueprintsAction, BlueprintsArgs};
use crate::blueprint;
use crate::brand;
use crate::errors::{AppError, AppResult};
use crate::utilities::{names, paths, text, ui::Ui};

pub fn run(args: BlueprintsArgs, ui: Ui) -> AppResult<()> {
    match args.action.unwrap_or(BlueprintsAction::List) {
        BlueprintsAction::List => list(ui),
        BlueprintsAction::New { name, from } => new(&name, &from, ui),
    }
}

fn list(ui: Ui) -> AppResult<()> {
    let project = paths::current_project_root()?;
    let blueprints = blueprint::list(project.as_deref())?;

    if ui.json {
        let items: Vec<serde_json::Value> = blueprints
            .iter()
            .map(|item| {
                serde_json::json!({
                    "name": item.name,
                    "description": item.description,
                    "source": item.source.label(),
                })
            })
            .collect();
        ui.json(&serde_json::Value::Array(items));
        return Ok(());
    }

    let mut rows = vec![vec![
        "NAME".to_string(),
        "FROM".to_string(),
        "DESCRIPTION".to_string(),
    ]];
    for item in &blueprints {
        rows.push(vec![
            item.name.clone(),
            item.source.label().to_string(),
            item.description.clone(),
        ]);
    }
    if !ui.quiet {
        print!("{}", text::table(&rows));
    }
    ui.hint(&format!(
        "\nMake your own:  {} blueprints new <name> --from <blueprint>",
        brand::NAME
    ));
    Ok(())
}

fn new(name: &str, from: &str, ui: Ui) -> AppResult<()> {
    names::check_blueprint_name(name).map_err(AppError::Usage)?;
    let Some(project) = paths::current_project_root()? else {
        return Err(AppError::NotFound(format!(
            "no {} project here. Run `{} init` first; custom blueprints live in the project's {}/blueprints/ folder.",
            brand::DISPLAY_NAME,
            brand::NAME,
            brand::DIR
        )));
    };
    if blueprint::find(name, Some(&project))?.is_some() {
        return Err(AppError::Usage(format!(
            "a blueprint called '{name}' already exists; pick another name"
        )));
    }
    if blueprint::find(from, Some(&project))?.is_none() {
        return Err(AppError::NotFound(format!(
            "there is no blueprint called '{from}'. Run `{} blueprints` to see them.",
            brand::NAME
        )));
    }

    let folder = blueprint::copy_to_project(from, name, &project)?;
    ui.done_with_note(
        &format!("created {}", paths::display_path(&folder)),
        &format!("copied from {from}"),
    );
    ui.hint(&format!(
        "Set `blueprint: {name}` in {}/vm.config.yaml to use it.",
        brand::DIR
    ));
    Ok(())
}
