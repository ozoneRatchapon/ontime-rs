import useScrollIntoView from '../../../../common/hooks/useScrollIntoView';
import { isWorkerBackend } from '../../../../externals';
import type { PanelBaseProps } from '../../panel-list/PanelList';
import * as Panel from '../../panel-utils/PanelUtils';
import QuickStart from '../../quick-start/QuickStart';
import type { SettingsOptionId } from '../../useAppSettingsMenu';
import ManageProjects from './ManageProjects';
import RoomProjectFile from './RoomProjectFile';

interface ProjectPanelProps extends PanelBaseProps {
  setLocation: (location: SettingsOptionId) => void;
}

export default function ProjectPanel({ location, setLocation }: ProjectPanelProps) {
  const manageProjectsRef = useScrollIntoView<HTMLDivElement>('list', location);

  const handleQuickClose = () => {
    setLocation('project');
  };

  // a room is the project: no list of files, only the file in and out (plan 020)
  if (isWorkerBackend) {
    return (
      <>
        <Panel.Header>Project</Panel.Header>
        <div ref={manageProjectsRef}>
          <RoomProjectFile />
        </div>
      </>
    );
  }

  return (
    <>
      <Panel.Header>Project</Panel.Header>
      {location === 'create' && <QuickStart isOpen onClose={handleQuickClose} />}
      <div ref={manageProjectsRef}>
        <ManageProjects />
      </div>
    </>
  );
}
