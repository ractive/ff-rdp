use ff_rdp_core::WindowGlobalTarget;
use serde_json::json;

use crate::cli::args::Cli;
use crate::error::AppError;
use crate::output;
use crate::output_pipeline::OutputPipeline;

use super::connect_tab::connect_and_get_target;

/// Which navigation action to perform.
#[derive(Clone, Copy)]
pub enum NavAction {
    Reload,
    Back,
    Forward,
}

pub fn run(cli: &Cli, action: NavAction) -> Result<(), AppError> {
    let mut ctx = connect_and_get_target(cli)?;
    let target_actor = ctx.target.actor.clone();

    let action_name = match action {
        NavAction::Reload => {
            WindowGlobalTarget::reload(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "reload"
        }
        NavAction::Back => {
            WindowGlobalTarget::go_back(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "back"
        }
        NavAction::Forward => {
            WindowGlobalTarget::go_forward(ctx.transport_mut(), &target_actor)
                .map_err(AppError::from)?;
            "forward"
        }
    };

    let result = json!({"action": action_name});
    let meta = json!({"host": cli.host, "port": cli.port});
    let envelope = output::envelope(&result, 1, &meta);

    OutputPipeline::new(cli.jq.clone())
        .finalize(&envelope)
        .map_err(AppError::from)
}
