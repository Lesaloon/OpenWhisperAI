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
pub enum ClipboardRestore {
    NotAttempted,
    Restored,
    Failed(ClipboardError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectResult {
    pub outcome: InjectOutcome,
    pub restore: ClipboardRestore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    ClipboardSet(ClipboardError),
    ClipboardPaste(ClipboardError),
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

    pub fn inject_text(&mut self, text: &str) -> Result<InjectResult, InjectError> {
        let previous = match self.clipboard.get_text() {
            Ok(value) => value,
            Err(err) => {
                return self
                    .typer
                    .type_text(text)
                    .map(|()| InjectResult {
                        outcome: InjectOutcome::TypedFallback,
                        restore: ClipboardRestore::NotAttempted,
                    })
                    .map_err(|typing_err| InjectError::Typing {
                        source: typing_err,
                        clipboard: Some(err),
                    });
            }
        };

        if let Err(err) = self.clipboard.set_text(text) {
            let restore_result = restore_clipboard(&mut self.clipboard, previous);
            return self.typing_fallback_after_restore(text, err, restore_result);
        }

        match self.clipboard.paste() {
            Ok(()) => {
                let restore_result = restore_clipboard(&mut self.clipboard, previous);
                Ok(InjectResult {
                    outcome: InjectOutcome::Clipboard,
                    restore: restore_outcome(restore_result),
                })
            }
            Err(paste_err) => self.typing_fallback_with_restore(text, paste_err, previous),
        }
    }

    fn typing_fallback_with_restore(
        &mut self,
        text: &str,
        clipboard_error: ClipboardError,
        previous: Option<String>,
    ) -> Result<InjectResult, InjectError> {
        let typing_result = self.typer.type_text(text);
        let restore_result = restore_clipboard(&mut self.clipboard, previous);

        match typing_result {
            Ok(()) => Ok(InjectResult {
                outcome: InjectOutcome::TypedFallback,
                restore: restore_outcome(restore_result),
            }),
            Err(typing_err) => {
                if let Err(restore_err) = restore_result {
                    return Err(InjectError::Typing {
                        source: typing_err,
                        clipboard: Some(restore_err),
                    });
                }

                Err(InjectError::Typing {
                    source: typing_err,
                    clipboard: Some(clipboard_error),
                })
            }
        }
    }

    fn typing_fallback_after_restore(
        &mut self,
        text: &str,
        clipboard_error: ClipboardError,
        restore_result: Result<(), ClipboardError>,
    ) -> Result<InjectResult, InjectError> {
        let typing_result = self.typer.type_text(text);

        match typing_result {
            Ok(()) => Ok(InjectResult {
                outcome: InjectOutcome::TypedFallback,
                restore: restore_outcome(restore_result),
            }),
            Err(typing_err) => Err(InjectError::Typing {
                source: typing_err,
                clipboard: Some(clipboard_error),
            }),
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

fn restore_outcome(result: Result<(), ClipboardError>) -> ClipboardRestore {
    match result {
        Ok(()) => ClipboardRestore::Restored,
        Err(err) => ClipboardRestore::Failed(err),
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
        fail_get: bool,
        fail_set: bool,
        fail_paste: bool,
        fail_clear: bool,
    }

    impl MockClipboard {
        fn new(content: Option<String>) -> Self {
            Self {
                content,
                ops: Vec::new(),
                fail_get: false,
                fail_set: false,
                fail_paste: false,
                fail_clear: false,
            }
        }
    }

    impl Clipboard for MockClipboard {
        fn get_text(&mut self) -> Result<Option<String>, ClipboardError> {
            self.ops.push(Op::Get);
            if self.fail_get {
                return Err(ClipboardError("get failed"));
            }
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

        assert_eq!(outcome.outcome, InjectOutcome::Clipboard);
        assert_eq!(outcome.restore, ClipboardRestore::Restored);
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

        assert_eq!(outcome.outcome, InjectOutcome::TypedFallback);
        assert_eq!(outcome.restore, ClipboardRestore::Restored);
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

        assert_eq!(outcome.outcome, InjectOutcome::TypedFallback);
        assert_eq!(
            outcome.restore,
            ClipboardRestore::Failed(ClipboardError("set failed"))
        );
        assert_eq!(clipboard.content, Some("keep".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![
                Op::Get,
                Op::Set("fallback".to_string()),
                Op::Set("keep".to_string()),
            ]
        );
        assert_eq!(typer.typed, vec!["fallback".to_string()]);
    }

    #[test]
    fn clears_clipboard_when_set_fails_and_previous_empty() {
        let mut clipboard = MockClipboard::new(None);
        clipboard.fail_set = true;
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("fallback").unwrap();
        let (clipboard, typer) = injector.into_parts();

        assert_eq!(outcome.outcome, InjectOutcome::TypedFallback);
        assert_eq!(outcome.restore, ClipboardRestore::Restored);
        assert_eq!(clipboard.content, None);
        assert_eq!(
            clipboard.ops,
            vec![Op::Get, Op::Set("fallback".to_string()), Op::Clear,]
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

        assert_eq!(outcome.outcome, InjectOutcome::Clipboard);
        assert_eq!(outcome.restore, ClipboardRestore::Restored);
        assert_eq!(clipboard.content, None);
        assert_eq!(
            clipboard.ops,
            vec![Op::Get, Op::Set("alpha".to_string()), Op::Paste, Op::Clear,]
        );
    }

    #[test]
    fn reports_restore_failure_after_successful_paste() {
        let mut clipboard = MockClipboard::new(None);
        clipboard.fail_clear = true;
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("alpha").unwrap();
        let (clipboard, _) = injector.into_parts();

        assert_eq!(outcome.outcome, InjectOutcome::Clipboard);
        assert_eq!(
            outcome.restore,
            ClipboardRestore::Failed(ClipboardError("clear failed"))
        );
        assert_eq!(clipboard.content, Some("alpha".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![Op::Get, Op::Set("alpha".to_string()), Op::Paste, Op::Clear,]
        );
    }

    #[test]
    fn falls_back_to_typing_when_get_text_fails() {
        let mut clipboard = MockClipboard::new(Some("keep".to_string()));
        clipboard.fail_get = true;
        let typer = MockTyper::default();
        let mut injector = Injector::new(clipboard, typer);

        let outcome = injector.inject_text("typed").unwrap();
        let (clipboard, typer) = injector.into_parts();

        assert_eq!(outcome.outcome, InjectOutcome::TypedFallback);
        assert_eq!(outcome.restore, ClipboardRestore::NotAttempted);
        assert_eq!(clipboard.content, Some("keep".to_string()));
        assert_eq!(clipboard.ops, vec![Op::Get]);
        assert_eq!(typer.typed, vec!["typed".to_string()]);
    }

    #[test]
    fn returns_typing_error_when_get_text_and_typing_fail() {
        let mut clipboard = MockClipboard::new(Some("keep".to_string()));
        clipboard.fail_get = true;
        let mut typer = MockTyper::default();
        typer.fail = true;
        let mut injector = Injector::new(clipboard, typer);

        let result = injector.inject_text("typed");
        let (clipboard, typer) = injector.into_parts();

        assert!(matches!(
            result,
            Err(InjectError::Typing {
                source: TypingError("typing failed"),
                clipboard: Some(ClipboardError("get failed")),
            })
        ));
        assert_eq!(clipboard.content, Some("keep".to_string()));
        assert_eq!(clipboard.ops, vec![Op::Get]);
        assert!(typer.typed.is_empty());
    }

    #[test]
    fn restores_clipboard_when_set_fails_and_typing_fails() {
        let mut clipboard = MockClipboard::new(Some("stash".to_string()));
        clipboard.fail_set = true;
        let mut typer = MockTyper::default();
        typer.fail = true;
        let mut injector = Injector::new(clipboard, typer);

        let result = injector.inject_text("fallback");
        let (clipboard, typer) = injector.into_parts();

        assert!(matches!(
            result,
            Err(InjectError::Typing {
                source: TypingError("typing failed"),
                clipboard: Some(ClipboardError("set failed")),
            })
        ));
        assert_eq!(clipboard.content, Some("stash".to_string()));
        assert_eq!(
            clipboard.ops,
            vec![
                Op::Get,
                Op::Set("fallback".to_string()),
                Op::Set("stash".to_string()),
            ]
        );
        assert!(typer.typed.is_empty());
    }

    #[test]
    fn restores_clipboard_when_paste_fails_and_typing_fails() {
        let mut clipboard = MockClipboard::new(Some("stash".to_string()));
        clipboard.fail_paste = true;
        let mut typer = MockTyper::default();
        typer.fail = true;
        let mut injector = Injector::new(clipboard, typer);

        let result = injector.inject_text("typed");
        let (clipboard, typer) = injector.into_parts();

        assert!(matches!(
            result,
            Err(InjectError::Typing {
                source: TypingError("typing failed"),
                clipboard: Some(ClipboardError("paste failed")),
            })
        ));
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
        assert!(typer.typed.is_empty());
    }
}
