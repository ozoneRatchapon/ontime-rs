import { recorder } from '@record';

const state = () => 'aux-state';
export const auxTimerService = {
  start: recorder('auxTimerService.start', state),
  pause: recorder('auxTimerService.pause', state),
  stop: recorder('auxTimerService.stop', state),
  setTime: recorder('auxTimerService.setTime', state),
  addTime: recorder('auxTimerService.addTime', state),
  setDirection: recorder('auxTimerService.setDirection', state),
};
