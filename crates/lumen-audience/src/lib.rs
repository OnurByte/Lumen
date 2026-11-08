//! Lower-trust chat data; operator commands never enter through this queue.
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub user: String,
    pub text: String,
    pub received_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Invalid,
    Duplicate,
    RateLimited,
    Expired,
    Full,
}

#[derive(Default)]
pub struct AudienceQueue {
    pending: VecDeque<Comment>,
    recent: VecDeque<(String, String, u64)>,
    last_user: Option<String>,
}

impl AudienceQueue {
    fn expire(&mut self, now_ms: u64) {
        self.pending
            .retain(|item| now_ms.saturating_sub(item.received_ms) <= 60_000);
        self.recent
            .retain(|(_, _, at)| now_ms.saturating_sub(*at) <= 60_000);
    }

    pub fn push(&mut self, comment: Comment, now_ms: u64) -> Result<(), Rejection> {
        if comment.id.is_empty()
            || comment.id.len() > 128
            || comment.user.trim().is_empty()
            || comment.user.len() > 128
            || comment.text.trim().is_empty()
            || comment.text.len() > 1000
            || comment.received_ms > now_ms
        {
            return Err(Rejection::Invalid);
        }
        if now_ms - comment.received_ms > 60_000 {
            return Err(Rejection::Expired);
        }
        self.expire(now_ms);
        if self.recent.iter().any(|(id, _, _)| id == &comment.id) {
            return Err(Rejection::Duplicate);
        }
        if self
            .recent
            .iter()
            .any(|(_, user, at)| user == &comment.user && now_ms.saturating_sub(*at) < 2000)
        {
            return Err(Rejection::RateLimited);
        }
        if self.pending.len() >= 128 || self.recent.len() >= 4096 {
            return Err(Rejection::Full);
        }
        self.recent
            .push_back((comment.id.clone(), comment.user.clone(), now_ms));
        self.pending.push_back(comment);
        Ok(())
    }

    pub fn next(&mut self, now_ms: u64) -> Option<Comment> {
        self.expire(now_ms);
        let index = self
            .pending
            .iter()
            .position(|comment| Some(&comment.user) != self.last_user.as_ref())
            .unwrap_or(0);
        let comment = self.pending.remove(index)?;
        self.last_user = Some(comment.user.clone());
        Some(comment)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}
