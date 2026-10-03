//! The host's user interface: prompts, and lines read from the person
//! at the console, through `$Host.UI`.
//!
//! Reached through [`crate::Pipeline::host_ui`], because a prompt runs
//! on the pipeline thread against the cmdlet's own host. Every call
//! here is a dynamic member access, which is what a prompt costs next
//! to the wait for a person. A host that cannot prompt, such as one
//! run with `-NonInteractive`, refuses with the engine's own error,
//! and that is the `Err`.

use crate::{ErrorCategory, FromPs, IntoPs, PsError, PsObject, PsResult, PsSecureString, PsType};

/// `$Host.UI` of the cmdlet's host.
pub struct HostUi {
    ui: PsObject,
}

impl HostUi {
    pub(crate) fn of(cmdlet: &PsObject) -> PsResult<HostUi> {
        Ok(HostUi { ui: cmdlet.get("Host")?.get("UI")? })
    }

    /// One line the person typed, without its line ending.
    pub fn read_line(&self) -> PsResult<String> {
        String::from_ps(&self.ui.call("ReadLine", &[])?)
    }

    /// One line typed without echo, as a `SecureString`.
    pub fn read_line_as_secure_string(&self) -> PsResult<PsSecureString> {
        PsSecureString::from_ps(&self.ui.call("ReadLineAsSecureString", &[])?)
    }

    /// Writes `text` and a line ending to the host's own output, which
    /// is not the pipeline and reaches no downstream command.
    pub fn write_line(&self, text: &str) -> PsResult<()> {
        self.ui.call("WriteLine", &[text.into_ps()?])?;
        Ok(())
    }

    /// Offers `choices`, each a label and a help text, under `caption`
    /// and `message`, and returns the index of the one chosen;
    /// `default` is the index taken on an empty answer. A `&` in a
    /// label marks its hot key, as the engine's own prompts do.
    pub fn prompt_for_choice(&self, caption: &str, message: &str, choices: &[(&str, &str)], default: usize) -> PsResult<usize> {
        // The engine's method binder converts an array of descriptions
        // to the Collection<ChoiceDescription> the method declares, as
        // it does for a script passing one.
        let mut descriptions = Vec::with_capacity(choices.len());
        for (label, help) in choices {
            descriptions.push(PsType::from_name("System.Management.Automation.Host.ChoiceDescription").new(&[label.into_ps()?, help.into_ps()?])?);
        }
        let chosen = self.ui.call("PromptForChoice", &[caption.into_ps()?, message.into_ps()?, descriptions.into_ps()?, (default as i32).into_ps()?])?;
        let index = i64::from_ps(&chosen)?;
        usize::try_from(index).map_err(|_| PsError::new(ErrorCategory::InvalidResult, "PwrsPromptChoice", format!("the host answered choice {index}")))
    }
}
