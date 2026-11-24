// @vitest-environment happy-dom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';

import { upload } from '../../../../../../common/api/excel';
import SourcesPanel from '../SourcesPanel';

vi.mock('../../../../../../common/api/excel', () => ({ upload: vi.fn() }));
vi.mock('../../../../../../common/hooks-query/useLoadedRundown', () => ({
  useLoadedRundown: () => ({ data: undefined }),
}));
vi.mock('../useSpreadsheetImport', () => ({ default: () => ({ applyImport: vi.fn() }) }));
vi.mock('react-router', () => ({ useNavigate: () => vi.fn() }));
vi.mock('../sheet-import/SheetImportEditor', () => ({ default: () => null }));

// the test mocks useLoadedRundown, which arrives with upstream 979de757 (not pulled yet);

// Excel upload is not offered by the cloud backend either

it.skip('shows the server explanation when an Excel upload is rejected', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.mocked(upload).mockRejectedValueOnce({
    isAxiosError: true,
    response: { statusText: 'Unprocessable Entity', data: { message: 'Invalid workbook. Replace the file.' } },
  });
  const container = document.createElement('div');
  const root = createRoot(container);
  try {
    await act(async () => root.render(<SourcesPanel />));
    const input = container.querySelector('input[type="file"]')!;
    Object.defineProperty(input, 'files', { value: [new File(['invalid ZIP'], 'invalid.xlsx')] });
    await act(async () => input.dispatchEvent(new Event('change', { bubbles: true })));

    expect(container.textContent).toContain('Invalid workbook. Replace the file.');
  } finally {
    await act(async () => root.unmount());
    vi.unstubAllGlobals();
  }
});
