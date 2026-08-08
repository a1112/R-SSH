import { describe, expect, test } from 'vitest';

import {
  encodeBinaryInput,
  encodeControl,
  encodeTerminalInput,
  parseServerMessage,
} from './protocol';

describe('web terminal protocol boundary', () => {
  test('encodes structured control messages', () => {
    expect(encodeControl({ type: 'resize', cols: 120, rows: 40 })).toBe(
      '{"type":"resize","cols":120,"rows":40}',
    );
  });

  test('accepts valid server messages and rejects malformed values', () => {
    expect(
      parseServerMessage(
        '{"type":"opened","protocol":1,"sessionId":"session","cols":80,"rows":24}',
      ),
    ).toEqual({ type: 'opened', protocol: 1, sessionId: 'session', cols: 80, rows: 24 });
    expect(parseServerMessage('{"type":"exit","code":"0","signal":null}')).toBeNull();
    expect(parseServerMessage('{not-json')).toBeNull();
  });

  test('preserves UTF-8 text and byte-oriented terminal input', () => {
    expect(Array.from(encodeTerminalInput('SSH 终端'))).toEqual(
      Array.from(new TextEncoder().encode('SSH 终端')),
    );
    expect(Array.from(encodeBinaryInput('\u0000\u00ff'))).toEqual([0, 255]);
  });
});
