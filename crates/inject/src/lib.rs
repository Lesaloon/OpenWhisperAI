#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(pub &'static str);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypingError(pub &'static str);

pub trait Clipboard {
    fn get_text(&mut self) -> Result<Option<String>, ClipboardError>;
    fn set_text(&mut self, text: &str) -> Result<(), ClipboardError>;
    fn clear(&mut self) -> Result<(), ClipboardError>;
    fn paste(&mut self) -> Result<(), ClipboardError>;
}

pub trait Typer {
    fn type_text(&mut self, text: &str) -> Result<(), TypingError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectOutcome {
    Clipboard,
    TypedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    ClipboardSet(ClipboardError),
    ClipboardPaste(ClipboardError),
    ClipboardRestore(ClipboardError),
    Typing {
        source: TypingError,
        clipboard: Option<ClipboardError>,
    },
}

pub struct Injector<C, T> {
    clipboard: C,
    typer: T,
}

impl<C, T> Injector<C, T>
where
    C: Clipboard,
    T: Typer,
{
    pub fn new(clipboard: C, typer: T) -> Self {
        Self { clipboard, typer }
    }

    pub fn into_parts(self) -> (C, T) {
        (self.clipboard, self.typer)
    }

    pub fn inject_text(&mut self, text: &str) -> Result<InjectOutcome, InjectError> {
        let previous = self.clipboard.get_text().unwrap_or(None);

        if let Err(err) = self.clipboard.set_text(text) {
            return self
                .typer
                .type_text(text)
                .map(|()| InjectOutcome::TypedFallback)
                .map_err(|typing_err| InjectError::Typing {
                    source: typing_err,
                    clipboard: Some(err),
                });
        }

        let paste_result = self.clipboard.paste();
        let restore_result = restore_clipboard(&mut self.clipboard, previous);

        match paste_result {
            Ok(()) => restore_result
                .map(|()| InjectOutcome::Clipboard)
                .map_err(InjectError::ClipboardRestore),
            Err(paste_err) => {
                self.typer
                    .type_text(text)
                    .map_err(|typing_err| InjectError::Typing {
                        source: typing_err,
                        clipboard: Some(paste_err.clone()),
                    })?;

                restore_result
                    .map(|()| InjectOutcome::TypedFallback)
                    .map_err(InjectError::ClipboardRestore)
            }
        }
    }
}

fn restore_clipboard<C: Clipboard>(
    clipboard: &mut C,
    previous: Option<String>,
) -> Result<(), ClipboardError> {
    match previous {
        Some(value) => clipboard.set_text(&value),
        None => clipboard.clear(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    enum Op {
        Get,
        Set(String),
        Clear,
        Paste,
    }

    struct MockClipboard {
        content: Option<String>,
        ops: Vec<Op>,
        fail_set: bool,
        fail_paste: bool,
        fail_clear: bool,
    }

    impl MockClipboard {
        fn new(content: Option<String>) -> Self {
            Self {
                content,
                ops: Vec::new(),
                fail_set: false,
                fail_paste: false,
                fail_clear: false,
            }
        }
    }

    impl Clipboard for MockClipboard {
        fn get_text(&mut self) -> Result<Option<String>, ClipboardError> {
            self.ops.push(Op::Get);
            Ok(self.content.clone())
        }

        fn set_text(&mut self, text: &str) -> Result<(), ClipboardError> {
            self.ops.push(Op::Set(text.to_string()));
            if self.fail_set {
                return Err(ClipboardError("set failed"));
            }
            self.content = Some(text.to_string());
            Ok(())
        }

        fn clear(&mut self) -> Result<(), ClipboardError> {
            self.ops.push(Op::Clear);
            if self.fail_clear {
                return Err(ClipboardError("clear failed"));
            }
            self.content = None;
            Ok(())
        }

        fn paste(&mut self) -> Result<(), ClipboardError> {
            self.ops.push(Op::Paste);
            if self.fail_paste {
                return Err(ClipboardError("paste failed"));
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockTyper {
        typed: Vec<String>,
        fail: bool,
    }

    impl Typer for MockTyper {
        fn type_text(&mut self, text: &str) -> Result<(), TypingError> {
            if self.fail {
                return Err(TypingError("typing failed"));
            }
            self.typed.push(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn restores_clipboard_after_successful_paste() {
        let clipboard = MockClipboard::new(Some("old".to_string()));
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("new").unwrap();
        let (clipboard, typer) = injector.into_parts();

        assert_eq!(outcome, InjectOutcome::Clipboard);
        assert_eq!(clipboard.content, Some("old".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![
                Op::Get,
                Op::Set("new".to_string()),
                Op::Paste,
                Op::Set("old".to_string()),
            ]
        );
        assert!(typer.typed.is_empty());
    }

    #[test]
    fn falls_back_to_typing_on_paste_failure_and_restores() {
        let mut clipboard = MockClipboard::new(Some("stash".to_string()));
        clipboard.fail_paste = true;
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("typed").unwrap();
        let (clipboard, typer) = injector.into_parts();

        assert_eq!(outcome, InjectOutcome::TypedFallback);
        assert_eq!(clipboard.content, Some("stash".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![
                Op::Get,
                Op::Set("typed".to_string()),
                Op::Paste,
                Op::Set("stash".to_string()),
            ]
        );
        assert_eq!(typer.typed, vec!["typed".to_string()]);
    }

    #[test]
    fn falls_back_to_typing_on_set_failure_without_paste() {
        let mut clipboard = MockClipboard::new(Some("keep".to_string()));
        clipboard.fail_set = true;
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("fallback").unwrap();
        let (clipboard, typer) = injector.into_parts();

        assert_eq!(outcome, InjectOutcome::TypedFallback);
        assert_eq!(clipboard.content, Some("keep".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![Op::Get, Op::Set("fallback".to_string()),]
        );
        assert_eq!(typer.typed, vec!["fallback".to_string()]);
    }

    #[test]
    fn clears_clipboard_when_previous_empty() {
        let clipboard = MockClipboard::new(None);
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("alpha").unwrap();
        let (clipboard, _) = injector.into_parts();

        assert_eq!(outcome, InjectOutcome::Clipboard);
        assert_eq!(clipboard.content, None);
        assert_eq!(
            clipboard.ops,
            vec![Op::Get, Op::Set("alpha".to_string()), Op::Paste, Op::Clear,]
        );
    }
}
