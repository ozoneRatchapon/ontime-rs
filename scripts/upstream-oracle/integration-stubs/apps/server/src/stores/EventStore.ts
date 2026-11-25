import { recorder } from '@record';

export const eventStore = { poll: recorder('eventStore.poll', () => 'poll-state') };
