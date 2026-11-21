//! 取消：每个任务一个标记，只停它自己。
//!
//! 下载和处理器到处都要看「是不是被取消了」，把标记一路传进每个函数签名太啰嗦，
//! 所以标记挂在线程上：任务线程用 [`spawn`] 或 [`with`] 带上自己的标记，
//! `check()` 看的是当前线程的标记。没挂标记的线程（图标、搜索这类后台查询）永远不会被取消，
//! 前台点「取消」也就不会误伤它们。
//!
//! 一次只有一个前台任务能被界面上的「取消」按钮停下：[`reset`] 给它发一个新标记，[`cancel`] 停它。
//! 任务里自己再开线程或线程池时，用 [`current`] 取出标记带过去，或者用 [`pool`] 建线程池。

use crate::error::{Error, Result};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// 一个任务的取消标记。克隆出来的是同一个标记。
#[derive(Debug, Clone, Default)]
pub struct Token(Arc<AtomicBool>);

impl Token {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

static FOREGROUND: Mutex<Option<Token>> = Mutex::new(None);

thread_local! {
    static CURRENT: RefCell<Option<Token>> = const { RefCell::new(None) };
}

fn foreground() -> std::sync::MutexGuard<'static, Option<Token>> {
    FOREGROUND
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 开始一个新的前台任务：返回它的标记，界面上的「取消」从此停的是它。
/// 把标记交给任务线程（[`spawn`]），否则任务看不到取消。
pub fn reset() -> Token {
    let token = Token::new();
    *foreground() = Some(token.clone());
    token
}

/// 停下当前的前台任务。
pub fn cancel() {
    if let Some(token) = foreground().as_ref() {
        token.cancel();
    }
}

/// 当前线程的标记；没挂标记时给一个永远不会被取消的。
pub fn current() -> Token {
    CURRENT.with(|current| current.borrow().clone().unwrap_or_default())
}

/// 带着 `token` 运行 `f`，结束后恢复原来的标记。
pub fn with<R>(token: Token, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Token>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|current| *current.borrow_mut() = previous);
        }
    }
    let previous = CURRENT.with(|current| current.borrow_mut().replace(token));
    let _restore = Restore(previous);
    f()
}

/// 开一个带着 `token` 的线程。
pub fn spawn<F, R>(token: Token, f: F) -> std::thread::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    std::thread::spawn(move || with(token, f))
}

/// 开一个带着当前标记的线程（任务里再开线程时用）。
pub fn spawn_here<F, R>(f: F) -> std::thread::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    spawn(current(), f)
}

/// `threads` 个线程的线程池，每个线程都带着当前的标记。
pub fn pool(threads: usize) -> std::result::Result<rayon::ThreadPool, rayon::ThreadPoolBuildError> {
    let token = current();
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .start_handler(move |_| {
            let token = token.clone();
            CURRENT.with(|current| *current.borrow_mut() = Some(token));
        })
        .build()
}

pub fn is_cancelled() -> bool {
    CURRENT.with(|current| current.borrow().as_ref().is_some_and(Token::is_cancelled))
}

pub fn check() -> Result<()> {
    if is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_and_cancel_round_trip() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let token = reset();
        with(token.clone(), || {
            assert!(!is_cancelled());
            assert!(check().is_ok());
            cancel();
            assert!(is_cancelled());
            assert!(matches!(check(), Err(Error::Cancelled)));
        });
        assert!(token.is_cancelled());
        // 不在任务里的线程不受影响。
        assert!(!is_cancelled());
        let next = reset();
        assert!(!next.is_cancelled());
    }

    #[test]
    fn cancelling_one_task_leaves_others_running() {
        let _guard = TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let install = reset();
        // 后台查询：没挂标记的线程。
        let background = std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(30));
            check().is_ok()
        });
        // 另一个自己带标记的任务。
        let other = Token::new();
        let other_task = spawn(other, || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            check().is_ok()
        });
        let foreground_task = spawn(install, || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            let nested = spawn_here(check).join().unwrap();
            let pooled = pool(2).unwrap().install(|| {
                use rayon::prelude::*;
                (0..4)
                    .into_par_iter()
                    .map(|_| check().is_err())
                    .collect::<Vec<_>>()
            });
            (
                check().is_err(),
                nested.is_err(),
                pooled.into_iter().all(|x| x),
            )
        });
        cancel();
        assert!(background.join().unwrap());
        assert!(other_task.join().unwrap());
        assert_eq!(foreground_task.join().unwrap(), (true, true, true));
        reset();
    }
}
