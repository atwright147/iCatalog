//! Native input preferences belong to the desktop shell.
use eframe::egui;

pub(super) fn configure(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    {
        // AppKit may return zero before a GUI session is initialized (headless
        // tests, for example). Use a usable desktop interval in that case.
        let seconds = objc2_app_kit::NSEvent::doubleClickInterval();
        set_double_click_interval(
            ctx,
            if seconds.is_finite() && seconds > 0.0 {
                seconds
            } else {
                0.5
            },
        );
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn set_double_click_interval(ctx: &egui::Context, seconds: f64) {
    if seconds.is_finite() && seconds > 0.0 {
        ctx.options_mut(|options| options.input_options.max_double_click_delay = seconds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_interval_is_applied_and_invalid_values_keep_the_previous_setting() {
        let ctx = egui::Context::default();
        set_double_click_interval(&ctx, 0.6);
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            set_double_click_interval(&ctx, value);
            assert_eq!(ctx.options(|o| o.input_options.max_double_click_delay), 0.6);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_the_system_double_click_interval() {
        let ctx = egui::Context::default();
        configure(&ctx);
        let interval = objc2_app_kit::NSEvent::doubleClickInterval();
        let interval = if interval.is_finite() && interval > 0.0 {
            interval
        } else {
            0.5
        };
        assert_eq!(
            ctx.options(|o| o.input_options.max_double_click_delay),
            interval
        );
    }
}
