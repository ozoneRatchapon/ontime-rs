import { ChangeEvent, useRef, useState } from 'react';

import { exportProjectFile, importProjectFile } from '../../../../common/api/db';
import { invalidateAllCaches, maybeAxiosError } from '../../../../common/api/utils';
import Button from '../../../../common/components/buttons/Button';
import useProjectData from '../../../../common/hooks-query/useProjectData';
import { validateProjectFile } from '../../../../common/utils/uploadUtils';
import { getRoom } from '../../../../externals';
import * as Panel from '../../panel-utils/PanelUtils';

/** A file name from the project title, or the room's name. */
export function projectFileName(title: string, room: string): string {
  const name = title.trim().replace(/[\\/:*?"<>|]+/g, '-');
  return name || room;
}

/**
 * On the Worker, a room is the project (plan 020). This moves its show to and
 * from the Ontime app: a file downloaded here opens in the app, and a file the
 * app saved takes the place of this room's show.
 */
export default function RoomProjectFile() {
  const { data } = useProjectData();
  const room = getRoom() ?? 'default';
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState<'import' | 'export' | null>(null);
  const [error, setError] = useState('');
  const [warnings, setWarnings] = useState<string[] | null>(null);

  const handleExport = async () => {
    setBusy('export');
    setError('');
    try {
      await exportProjectFile(projectFileName(data.title, room));
    } catch (err) {
      setError(`Could not download the project file: ${maybeAxiosError(err)}`);
    } finally {
      setBusy(null);
    }
  };

  const handleImport = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    // the same file chosen twice is a change too
    event.target.value = '';
    if (!file) return;
    const replace = window.confirm(
      `Import ${file.name}?\n\nIts rundowns and settings take the place of this room's show. ` +
        'The room keeps its link, members and report history, and its undo history keeps a copy of the show it replaces.',
    );
    if (!replace) return;

    setBusy('import');
    setError('');
    setWarnings(null);
    try {
      validateProjectFile(file);
      const result = await importProjectFile(file);
      setWarnings(result.warnings);
    } catch (err) {
      setError(`Could not import the file: ${maybeAxiosError(err)}`);
    } finally {
      await invalidateAllCaches();
      setBusy(null);
    }
  };

  return (
    <Panel.Section>
      <input
        ref={fileInputRef}
        style={{ display: 'none' }}
        type='file'
        onChange={handleImport}
        accept='.json'
        data-testid='file-input'
      />
      <Panel.Card>
        <Panel.SubHeader>
          Project file
          <Panel.InlineElements>
            <Button onClick={() => fileInputRef.current?.click()} disabled={Boolean(busy)} loading={busy === 'import'}>
              Import
            </Button>
            <Button onClick={handleExport} disabled={Boolean(busy)} loading={busy === 'export'}>
              Download
            </Button>
          </Panel.InlineElements>
        </Panel.SubHeader>
        <Panel.Description>
          Move this show to and from the Ontime app. A downloaded file opens in the app; a file saved by the app
          replaces the show in room <Panel.Highlight>{room}</Panel.Highlight>.
        </Panel.Description>
        {error && <Panel.Error>{error}</Panel.Error>}
        {warnings && warnings.length === 0 && <Panel.Description>Imported.</Panel.Description>}
        {warnings && warnings.length > 0 && (
          <>
            <Panel.Description tone='warning'>
              Imported. Parts of the file were left out or defaulted:
            </Panel.Description>
            <Panel.ListGroup>
              {warnings.map((warning) => (
                <Panel.ListItem key={warning}>{warning}</Panel.ListItem>
              ))}
            </Panel.ListGroup>
          </>
        )}
      </Panel.Card>
    </Panel.Section>
  );
}
