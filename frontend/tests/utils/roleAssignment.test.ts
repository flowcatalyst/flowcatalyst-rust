import { describe, expect, it } from 'vitest';
import { assignmentSourceSeverity } from '../../src/utils/roleAssignment';

describe('assignmentSourceSeverity', () => {
  it('highlights roles an admin assigned, including legacy ADMIN rows', () => {
    expect(assignmentSourceSeverity('ADMIN_ASSIGNED')).toBe('info');
    expect(assignmentSourceSeverity('ADMIN')).toBe('info');
  });

  it('mutes synced and platform-granted roles', () => {
    for (const source of ['IDP_SYNC', 'SDK_SYNC', 'PROVISIONED', 'BOOTSTRAP', 'SYSTEM']) {
      expect(assignmentSourceSeverity(source)).toBe('secondary');
    }
  });
});
