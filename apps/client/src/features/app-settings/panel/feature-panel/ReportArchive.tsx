import { ArchivedRun } from 'ontime-types';
import { millisToString } from 'ontime-utils';

import useReportArchive from '../../../../common/hooks-query/useReportArchive';
import { formatTime } from '../../../../common/utils/time';
import * as Panel from '../../panel-utils/PanelUtils';

/**
 * Shows the permanent run archive: every run that was explicitly saved, kept for
 * year-over-year analysis. Unlike the live report — which is cleared when a run
 * is archived so the next run starts clean — this survives edits and deletions,
 * because each event's title, cue and planned duration are copied in at save time.
 */
export default function ReportArchive() {
  const { data: runs } = useReportArchive();

  return (
    <Panel.Section>
      <Panel.Card>
        <Panel.SubHeader>Run archive</Panel.SubHeader>
        <Panel.Description>
          Every run you saved, kept permanently — planned against the time each segment actually ran.
        </Panel.Description>
        <Panel.Divider />
        {runs.length === 0 ? (
          <Panel.Paragraph>
            No runs archived yet. Save a finished run (POST /data/report/archive) to keep its timing here.
          </Panel.Paragraph>
        ) : (
          runs.map((run, index) => <ArchivedRunTable key={`${run.rundownId}-${run.archivedAt}-${index}`} run={run} />)
        )}
      </Panel.Card>
    </Panel.Section>
  );
}

function ArchivedRunTable({ run }: { run: ArchivedRun }) {
  return (
    <Panel.Section>
      <Panel.Title>{run.rundownTitle}</Panel.Title>
      <Panel.Table>
        <thead>
          <tr>
            <th>#</th>
            <th>Cue</th>
            <th>Title</th>
            <th>Planned</th>
            <th>Actual start</th>
            <th>Actual end</th>
            <th>Diff</th>
          </tr>
        </thead>
        <tbody>
          {run.events.map((event, index) => {
            const actual =
              event.startedAt != null && event.endedAt != null ? event.endedAt - event.startedAt : null;
            const diff = actual != null ? actual - event.plannedDuration : null;
            const diffLabel =
              diff === null ? '—' : diff === 0 ? 'on time' : `${diff > 0 ? '+' : '-'}${millisToString(Math.abs(diff))}`;
            return (
              <tr key={event.id}>
                <th>{index + 1}</th>
                <th>{event.cue}</th>
                <th>{event.title}</th>
                <th>{millisToString(event.plannedDuration)}</th>
                <th>{formatTime(event.startedAt)}</th>
                <th>{formatTime(event.endedAt)}</th>
                <th>{diffLabel}</th>
              </tr>
            );
          })}
        </tbody>
      </Panel.Table>
    </Panel.Section>
  );
}
