import type { Playback } from '../../definitions/runtime/Playback.type.js';
import type { MaybeString } from '../../utils/utils.type.js';

export type NetworkInterface = {
  name: string;
  address: string;
};

export type PortInfo = {
  port: number;
  pendingRestart: boolean;
};

export interface SessionStats {
  startedAt: string;
  connectedClients: number;
  lastConnection: MaybeString;
  lastRequest: MaybeString;
  projectName: string;
  playback: Playback;
  timezone: string;
  version: string;
  nodeId: string;
}

export interface GetInfo {
  networkInterfaces: NetworkInterface[];
  version: string;
  serverPort: number;
  publicDir: string;
  /** Whether the current room requires a token. Absent on older backends. */
  protected?: boolean;
  /** Whether only members may edit this room. Absent on older backends. */
  membersOnly?: boolean;
  /** Whether this caller may edit the room. Absent on older backends. */
  canEdit?: boolean;
}

export interface GetUrl {
  url: string;
}

export type ProjectFile = {
  filename: string;
  updatedAt: string;
};

export type ProjectFileList = ProjectFile[];

export type ProjectFileListResponse = {
  files: ProjectFileList;
  lastLoadedProject: string;
};

export type MessageResponse = {
  message: string;
};

export type ProjectLogoResponse = {
  logoFilename: string;
};

export type ErrorResponse = MessageResponse;

export type AuthenticationStatus = 'authenticated' | 'not_authenticated' | 'pending';
