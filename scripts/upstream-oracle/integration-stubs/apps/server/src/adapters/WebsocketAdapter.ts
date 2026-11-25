import { recorder } from '@record';

export const socket = {
  renameClient: recorder('socket.renameClient'),
  redirectClient: recorder('socket.redirectClient'),
  identifyClient: recorder('socket.identifyClient'),
};
