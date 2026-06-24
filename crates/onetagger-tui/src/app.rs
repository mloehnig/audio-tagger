/// Screens in the app. SP1 only navigates Home -> Dashboard; other variants land in later SPs.
pub enum Screen {
    Home,
    AutotaggerForm,
    Dashboard,
}

/// Messages a screen's key handler can return; applied centrally by `App`.
pub enum Action {
    None,
    Push(Screen),
    Pop,
    Quit,
}

pub struct App {
    pub stack: Vec<Screen>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> App {
        App { stack: vec![Screen::Home], should_quit: false }
    }

    pub fn current(&self) -> &Screen {
        self.stack.last().unwrap_or(&Screen::Home)
    }

    pub fn apply_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Push(screen) => self.stack.push(screen),
            Action::Pop => {
                self.stack.pop();
                if self.stack.is_empty() {
                    self.should_quit = true;
                }
            }
            Action::Quit => self.should_quit = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_home() {
        let app = App::new();
        assert!(matches!(app.current(), Screen::Home));
        assert!(!app.should_quit);
    }

    #[test]
    fn push_and_pop() {
        let mut app = App::new();
        app.apply_action(Action::Push(Screen::Dashboard));
        assert!(matches!(app.current(), Screen::Dashboard));
        app.apply_action(Action::Pop);
        assert!(matches!(app.current(), Screen::Home));
    }

    #[test]
    fn pop_at_home_quits() {
        let mut app = App::new();
        app.apply_action(Action::Pop);
        assert!(app.should_quit);
    }

    #[test]
    fn quit_action() {
        let mut app = App::new();
        app.apply_action(Action::Quit);
        assert!(app.should_quit);
    }
}
