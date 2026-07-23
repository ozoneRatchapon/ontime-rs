import axios from 'axios';

import { getToken, serverURL } from '../../externals';
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
  const token = getToken();
  if (!token) {
    return config;
  }

  const url = config.url ?? '';
  const isOurBackend = url.startsWith(serverURL) || url.startsWith('/');
  if (isOurBackend) {
    config.headers.set('Authorization', `Bearer ${token}`);
  }

  return config;
});
