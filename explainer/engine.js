// Byte-level bridge to the grading-core shim. `ex` is either the WebAssembly
// instance exports or the wasm2js module namespace; both expose the same API.
function makeEngine(ex) {
  const mem = () => new Uint8Array(ex.memory.buffer);
  const put = (bytes) => {
    const p = ex.q_alloc(bytes.length || 1);
    mem().set(bytes, p);
    return p;
  };
  const take = (p, n) => mem().slice(p, p + n);
  const free = (p, n) => ex.q_free(p, n || 1);

  const keyBytes = (key) => {
    const out = [key.questions.length, key.numChoices, key.policy === "redistribute" ? 1 : 0];
    for (const q of key.questions) {
      let mask = 0;
      for (const c of q.accepted) mask |= 1 << c;
      out.push(q.weight, q.cancelled ? 1 : 0, mask);
    }
    return Uint8Array.from(out);
  };
  const sheetBytes = (sheet) => {
    const b = new Uint8Array(32 + sheet.answers.length);
    b.set(hexToBytes(sheet.pseudonym), 0);
    sheet.answers.forEach((a, i) => (b[32 + i] = a === null ? 0xff : a));
    return b;
  };

  function withBufs(inputs, outLen, fn) {
    const ptrs = inputs.map((b) => (b ? put(b) : 0));
    const out = ex.q_alloc(outLen);
    try {
      const rc = fn(ptrs, out);
      return { rc, bytes: take(out, outLen) };
    } finally {
      inputs.forEach((b, i) => b && free(ptrs[i], b.length));
      free(out, outLen);
    }
  }

  return {
    commit(key, salt) {
      const k = keyBytes(key);
      const r = withBufs([k, salt], 32, ([pk, ps], o) => ex.q_commit(pk, k.length, ps, o));
      if (r.rc !== 0) throw new Error("commit failed " + r.rc);
      return r.bytes;
    },
    sheetHash(sheet) {
      const s = sheetBytes(sheet);
      const r = withBufs([s], 32, ([p], o) => ex.q_sheet_hash(p, s.length, o));
      if (r.rc !== 0) throw new Error("sheet hash failed " + r.rc);
      return r.bytes;
    },
    grade(key, salt, sheet) {
      const k = keyBytes(key), s = sheetBytes(sheet);
      const r = withBufs([k, salt, s], 92, ([pk, ps, psh], o) => ex.q_grade(pk, k.length, ps, psh, s.length, o));
      if (r.rc !== 0) throw new Error("grade failed " + r.rc);
      return r.bytes;
    },
    gradeBatch(key, salt, sheets) {
      const k = keyBytes(key);
      const sb = sheets.map(sheetBytes);
      const len = sb[0].length;
      const all = new Uint8Array(len * sb.length);
      sb.forEach((b, i) => all.set(b, i * len));
      const cap = 76 + sheets.length * (92 + 32 + 1 + 32 * 24);
      const r = withBufs([k, salt, all], cap, ([pk, ps, pa], o) =>
        ex.q_grade_batch(pk, k.length, ps, pa, len, sheets.length, o, cap));
      if (r.rc < 0) throw new Error("batch failed " + r.rc);
      const b = r.bytes;
      const n = sheets.length;
      let off = 0;
      const batchPv = b.slice(0, 76); off = 76;
      const reports = [];
      for (let i = 0; i < n; i++, off += 92) reports.push(b.slice(off, off + 92));
      const leaves = [];
      for (let i = 0; i < n; i++, off += 32) leaves.push(b.slice(off, off + 32));
      const paths = [];
      for (let i = 0; i < n; i++) {
        const c = b[off++];
        const sib = [];
        for (let j = 0; j < c; j++, off += 32) sib.push(b.slice(off, off + 32));
        paths.push(sib);
      }
      return { batchPv, root: batchPv.slice(32, 64), reports, leaves, paths };
    },
    leafHash(reportPv) {
      const r = withBufs([reportPv], 32, ([p], o) => ex.q_leaf_hash(p, o));
      if (r.rc !== 0) throw new Error("leaf hash failed " + r.rc);
      return r.bytes;
    },
    checkInclusion(batchPv, commitment, reportPv, index, siblings, sheetHash) {
      const sib = new Uint8Array(32 * siblings.length);
      siblings.forEach((s, i) => sib.set(s, 32 * i));
      return withBufs([batchPv, commitment, reportPv, sib.length ? sib : null, sheetHash || null], 1,
        ([pb, pc, pr, psib, psh]) => ex.q_check_inclusion(pb, pc, pr, index, psib, siblings.length, psh)).rc;
    },
  };
}

function hexToBytes(h) {
  const b = new Uint8Array(h.length / 2);
  for (let i = 0; i < b.length; i++) b[i] = parseInt(h.substr(i * 2, 2), 16);
  return b;
}
function bytesToHex(b) {
  return Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
}
function readScore(pv) {
  const dv = new DataView(pv.buffer, pv.byteOffset, pv.byteLength);
  return { scoreBp: dv.getUint32(72, true), correct: dv.getUint32(76, true), wrong: dv.getUint32(80, true),
           blank: dv.getUint32(84, true), cancelled: dv.getUint32(88, true) };
}
function withScore(pv, scoreBp) {
  const out = pv.slice();
  new DataView(out.buffer).setUint32(72, scoreBp, true);
  return out;
}

if (typeof module !== "undefined") module.exports = { makeEngine, hexToBytes, bytesToHex, readScore, withScore };
