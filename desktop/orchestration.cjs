/**
 * CodeBridge multi-model orchestration.
 * A: implement -> independent review -> implement fixes.
 * B: parallel READ-ONLY independent investigations -> single-writer integration.
 * C: independent proposals -> mutual critique -> synthesis.
 * OpenCode remains the sole executor of tools. No second LLM is embedded.
 */
'use strict';

const { randomUUID } = require('crypto');
const TERMINAL = new Set(['stop', 'length', 'error', 'content-filter']);
const SUPPORTED = new Set(['webchat-chatgpt', 'webchat-gemini']);
const pause = (ms) => new Promise((r) => setTimeout(r, ms));

function unwrap(r, action) {
  if (!r || r.status < 200 || r.status >= 300 || r.json?.error)
    throw new Error(`${action}: ${r?.error || r?.json?.error?.message || r?.body || r?.status || 'no response'}`);
  return r.json?.data ?? r.json;
}
function textFrom(messages) {
  const assistant = [...messages].reverse().find((m) => m.type === 'assistant' || m.info?.role === 'assistant');
  if (!assistant) return { assistant: null, text: '', finish: null };
  const parts = assistant.content || assistant.parts || [];
  const text = parts.filter((p) => p.type === 'text').map((p) => p.text || '').join('\n');
  const finish = assistant.finish || assistant.info?.finish || assistant.info?.finishReason;
  return { assistant, text, finish };
}

function createOrchestrator(ocFetch, notify = () => {}, opts = {}) {
  const jobs = new Map();
  const pollMs = opts.pollMs ?? 1200;
  const timeoutMs = opts.timeoutMs ?? 300000;
  const limitText = (s) => String(s || '').slice(0, 28000);

  async function runAgent(job, model, prompt, label) {
    if (job.cancelled) throw new Error('사용자가 작업을 중단했습니다.');
    if (!SUPPORTED.has(model)) throw new Error(`현재 연결되지 않은 모델: ${model}`);
    update(job, label, 'running', model);
    const dir = job.directory;
    const created = unwrap(await ocFetch('/api/session', {
      method: 'POST', body: { location: { directory: dir }, model: { id: model, providerID: 'webchat' } },
    }), '세션 생성');
    const id = created.id;
    if (!id) throw new Error('OpenCode 세션 ID를 받지 못했습니다.');
    job.sessions.push(id);
    const baselineRes = unwrap(await ocFetch(`/api/session/${encodeURIComponent(id)}/message?order=asc&limit=200`), '기존 메시지 조회');
    const baseline = new Set((Array.isArray(baselineRes) ? baselineRes : []).map((m) => m.id));
    const sent = await ocFetch(`/api/session/${encodeURIComponent(id)}/prompt`, { method: 'POST', body: { text: prompt } });
    unwrap(sent, '프롬프트 전송');
    const deadline = Date.now() + timeoutMs;
    let latest = '';
    while (Date.now() < deadline) {
      if (job.cancelled) throw new Error('사용자가 작업을 중단했습니다.');
      const data = unwrap(await ocFetch(`/api/session/${encodeURIComponent(id)}/message?order=asc&limit=200`), '메시지 조회');
      const messages = Array.isArray(data) ? data : [];
      const newly = messages.filter((m) => !baseline.has(m.id));
      const { assistant, text, finish } = textFrom(newly);
      if (text) latest = text;
      if (assistant && TERMINAL.has(finish)) {
        if (finish !== 'stop') throw new Error(`${label}: 모델 응답 종료 사유 ${finish}`);
        if (!latest.trim()) throw new Error(`${label}: 응답 텍스트가 없습니다.`);
        update(job, label, 'completed', model);
        return limitText(latest);
      }
      await pause(pollMs);
    }
    throw new Error(`${label}: 응답 대기 시간 초과 (세션 ${id})`);
  }

  function update(job, label, status, model) {
    job.events.push({ time: Date.now(), label, status, model });
    job.updatedAt = Date.now();
    notify(job.id, { label, status, model });
  }

  async function execute(job) {
    const { prompt, primary, secondary, mode, tasks } = job;
    const reading = '읽기 전용 조사입니다. 어떤 파일도 수정하지 말고 셸의 변경 명령도 실행하지 마세요. 확인한 코드와 근거만 보고하세요.';
    const p = async (model, input, label) => runAgent(job, model, input, label);
    if (mode === 'review') {
      const implementation = await p(primary,
        `다음 요청을 OpenCode 도구로 실제 구현하고 테스트하세요. 수행 내역과 파일 변경 및 테스트 결과를 보고하세요.\n\n${prompt}`, 'A · 구현');
      const review = await p(secondary,
        `${reading}\n\n원래 사용자 요청:\n${prompt}\n\n구현 에이전트 보고:\n${implementation}\n\n현재 프로젝트를 읽고 구현의 오류·보안·테스트 누락을 독립 검증하세요. 근거 없는 판단은 하지 마세요.`, 'A · 독립 검토');
      const result = await p(primary,
        `원래 요청:\n${prompt}\n\n독립 리뷰:\n${review}\n\n리뷰에서 검증된 지적 사항을 수정하고 테스트하세요. 반박 가능한 지적은 증거를 제시하세요. 최종 결과를 보고하세요.`, 'A · 수정 및 검증');
      return { implementation, review, result };
    }
    if (mode === 'parallel') {
      if (!Array.isArray(tasks) || tasks.length < 2 || tasks.length > 4 || tasks.some((s) => !String(s).trim()))
        throw new Error('B 모드에는 독립적인 하위 작업을 2~4개 지정해야 합니다.');
      // Read-only workers can share the project safely; a single integrator owns writes.
      const findings = await Promise.all(tasks.map((task, i) => p(i % 2 ? secondary : primary,
        `${reading}\n\n전체 목표:\n${prompt}\n\n담당 하위 작업 ${i + 1}:\n${task}\n\n구현을 위한 분석과 제안, 정확한 파일 경로 및 테스트 항목을 보고하세요.`,
        `B · 병렬 조사 ${i + 1}`)));
      const result = await p(primary,
        `다음 목표를 실제 구현하세요:\n${prompt}\n\n독립 하위 작업 조사 결과:\n${findings.map((f, i) => `[조사 ${i + 1}: ${tasks[i]}]\n${f}`).join('\n\n')}\n\n파일 충돌을 피하기 위해 한 에이전트로 통합 구현하고 테스트 결과를 보고하세요.`,
        'B · 통합 구현 및 검증');
      return { findings, result };
    }
    if (mode === 'debate') {
      const proposals = await Promise.all([primary, secondary].map((m, i) => p(m,
        `${reading}\n\n다음 문제에 대해 독립적인 설계안과 근거, 위험을 제시하세요. 다른 모델이 제안했을 법한 답변을 가정하지 마세요.\n\n${prompt}`, `C · 독립 제안 ${i + 1}`)));
      const critiques = await Promise.all([primary, secondary].map((m, i) => p(m,
        `${reading}\n\n문제:\n${prompt}\n\n내 초기안:\n${proposals[i]}\n\n상대 모델의 실제 제안:\n${proposals[1 - i]}\n\n서로의 장단점을 비교하고 사실 확인이 필요한 주장을 구분하세요. 수정된 결론을 제시하세요.`, `C · 교차 토론 ${i + 1}`)));
      const result = await p(primary,
        `${reading}\n\n문제:\n${prompt}\n\n독립 제안:\n${proposals.join('\n\n---\n\n')}\n\n교차 검토:\n${critiques.join('\n\n---\n\n')}\n\n검증 가능한 증거를 기준으로 합의점·이견·최종 제안과 실행 계획을 정리하세요. 이 단계에서는 코드를 수정하지 마세요.`, 'C · 최종 종합');
      return { proposals, critiques, result };
    }
    throw new Error('알 수 없는 실행 모드');
  }

  function start(args) {
    const { mode, prompt, directory, primary, secondary, tasks = [] } = args || {};
    if (!['review', 'parallel', 'debate'].includes(mode)) throw new Error('잘못된 모드');
    if (typeof prompt !== 'string' || !prompt.trim() || prompt.length > 12000) throw new Error('요청은 1~12000자여야 합니다.');
    if (typeof directory !== 'string' || !directory.trim()) throw new Error('프로젝트 폴더를 선택하세요.');
    if (!SUPPORTED.has(primary) || !SUPPORTED.has(secondary)) throw new Error('지원되지 않는 모델');
    if (primary === secondary) throw new Error('독립 검토를 위해 서로 다른 모델을 선택하세요.');
    const id = randomUUID();
    const job = { id, mode, prompt, directory, primary, secondary, tasks, status: 'running',
      events: [], sessions: [], result: null, error: null, cancelled: false,
      createdAt: Date.now(), updatedAt: Date.now() };
    jobs.set(id, job);
    void execute(job).then((result) => {
      job.result = result;
      job.status = 'completed';
      update(job, '작업 완료', 'completed', null);
    }).catch((error) => {
      job.status = job.cancelled ? 'cancelled' : 'error';
      job.error = String(error?.message || error);
      update(job, '작업 종료', job.status, null);
    });
    return { id };
  }

  function get(id) {
    const job = jobs.get(id);
    if (!job) return null;
    const { prompt, directory, tasks, ...safe } = job;
    return { ...safe, tasks };
  }
  async function cancel(id) {
    const job = jobs.get(id);
    if (!job) return false;
    job.cancelled = true;
    for (const session of job.sessions) {
      await ocFetch(`/api/session/${encodeURIComponent(session)}/interrupt`, { method: 'POST', body: {} }).catch(() => {});
    }
    return true;
  }
  return { start, get, cancel, _execute: execute };
}
module.exports = { createOrchestrator, textFrom, unwrap };
