//! 컨텍스트 평탄화의 예산 정책 (Phase 1).
//! flatten.rs가 자체 예산 로직을 갖고 있어, 이 모듈은 보조 유틸만 둔다.
#![allow(dead_code)]

pub fn approx_tokens(text: &str) -> usize {
    // 한글/영문 혼합 기준 문자수/3.5 근사 (정확한 토크나이저 없이 상한만 본다)
    (text.chars().count() as f64 / 3.5).ceil() as usize
}
