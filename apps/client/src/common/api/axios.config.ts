import axios from 'axios';

import { getPersonSession, getToken, serverURL } from '../../externals';
import { axiosConfig } from './requestTimeouts';

axios.defaults.validateStatus = (status) => {
  return status >= 200 && status < 300;
};
axios.defaults.timeout = axiosConfig.shortTimeout;

/**
 * Attaches the room token to backend requests.
 *
 * Scoped to our own server URL so the token is never sent to third parties the app
 * also talks to (the GitHub releases endpoint, for one).
 */
axios.interceptors.request.use((config) => {
  const url = config.url ?? '';
  const isOurBackend = url.startsWith(serverURL) || url.startsWith('/');
  if (!isOurBackend) {
    return config;
  }

  const token = getToken();
  if (token) {
    config.headers.set('Authorization', `Bearer ${token}`);
  }
  // a signed-in member reaches their rooms without the room password
  const session = getPersonSession();
  if (session) {
    config.headers.set('X-Ontime-Session', session);
  }

  return config;
});
