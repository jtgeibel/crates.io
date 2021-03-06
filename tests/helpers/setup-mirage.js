import setupMirage from 'ember-cli-mirage/test-support/setup-mirage';
import window from 'ember-window-mock';
import { setupWindowMock } from 'ember-window-mock/test-support';
import timekeeper from 'timekeeper';

export default function (hooks) {
  setupMirage(hooks);
  setupWindowMock(hooks);

  // To have deterministic visual tests, the seed has to be constant
  hooks.beforeEach(function () {
    timekeeper.travel(new Date('2017-11-20T12:00:00'));

    this.authenticateAs = user => {
      this.server.create('mirage-session', { user });
      window.localStorage.setItem('isLoggedIn', '1');
    };
  });

  hooks.afterEach(function () {
    timekeeper.reset();
  });
}
