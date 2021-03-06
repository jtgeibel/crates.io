import { currentURL, fillIn, triggerEvent, visit } from '@ember/test-helpers';
import { module, test } from 'qunit';

import { setupApplicationTest } from 'cargo/tests/helpers';

module('Acceptance | 404', function (hooks) {
  setupApplicationTest(hooks);

  test('/unknown-route shows a 404 page', async function (assert) {
    await visit('/unknown-route');
    assert.equal(currentURL(), '/unknown-route');
    assert.dom('[data-test-404-header]').exists();
  });

  test('search on the 404 page works correctly', async function (assert) {
    await visit('/unknown-route');
    assert.dom('[data-test-404-search-input]').hasValue('');

    await fillIn('[data-test-404-search-input]', 'rust');
    assert.dom('[data-test-404-search-input]').hasValue('rust');

    await triggerEvent('[data-test-404-search-form]', 'submit');
    assert.equal(currentURL(), '/search?q=rust');
  });
});
