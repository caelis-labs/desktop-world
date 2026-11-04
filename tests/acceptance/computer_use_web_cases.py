"""Independent WebKit extended cases for CoreCases.

The caller supplies a fresh fixture per method and owns capture/cursor/focus auditing.
All actions use apps/exec. Disk state and published geometry are supervisor oracles;
geometry-backed pixel actions do not claim model vision. No write is retried.
An ``unavailable`` or ``blocked`` result must be recorded as such by the runner.
"""
import copy
import json

from app_client import require
from standard_agent_probe import wait_until


class WebExtendedCases:
    def _web_click(self, text):
        role = 'link' if text in ('Canvas chat', 'Inventory') else 'button'
        return self.run('await app.click(' + json.dumps({'selector': {'role': role, 'text': text}}) + ');')

    def _web_field(self, text):
        names = {'Review note': 'review-note', 'Review quantity': 'review-number',
                 'Approved': 'review-approved', 'Review date': 'review-date',
                 'Search catalogue': 'catalogue-search'}
        if text in names:
            point = self._web_point(names[text], 'web-field-' + names[text])
        else:
            point = self._workbench_field_point(text)
        return self.run('await app.click(' + json.dumps(point) + ');')

    def _web_type(self, text, value):
        observed = self.run('const v=await app.look({role:"text_field",text:' + json.dumps(text)
                            + '}); return {elements:v.elements};')
        fields = [e for e in observed.get('result', {}).get('elements', [])
                  if e.get('role') == 'text_field' and e.get('name') == text]
        if len(fields) == 1 and 'press' in fields[0].get('actions', []):
            self.run('await app.click({target:' + json.dumps(fields[0]['id']) + '});')
        else:
            self._web_field(text)
        return self.run("await app.keypress('META+A'); await app.typeText(" + json.dumps(value) + ');')

    def _web_point(self, name, capture):
        from computer_use_benchmark import image_point
        web = wait_until(lambda: s if name in (s := self.fixture.web_state())['layout'].get('anchors', {}) else None)
        view = self.capture(capture)
        # Capture may compose several windows: image.bounds is its full screen
        # rectangle, never the DOM viewport or an individual modal rectangle.
        web = self.fixture.web_state()
        origin = self.state()['anchors']['web-origin']
        local = web['layout']['anchors'][name]
        width, height = web['layout']['viewport']
        require(0 <= local[0] < width and 0 <= local[1] < height,
                {'reason': 'Web field is outside the current viewport', 'anchor': name, 'layout': web['layout']})
        return image_point(view, [origin[0] + local[0], origin[1] + local[1]])

    def _workbench_field_point(self, label):
        from computer_use_benchmark import image_point
        require(label in ('Target quantity', 'Handoff note', 'Reviewed'), label)
        view = self.capture('workbench-field-' + label.replace(' ', '-'))
        state = self.state()
        origin = state['anchors']['web-origin']
        # The reused workbench has no geometry oracle. Its HTML dialog is centered
        # in this same WebKit viewport, retained from the owned entry page. These
        # interior points follow index.html's existing 460px dialog and field CSS.
        width, height = self.fixture.web_state()['layout']['viewport']
        windows = [w for w in state['windows'] if
                   w['bounds'][0] <= origin[0] < w['bounds'][0] + w['bounds'][2] and
                   w['bounds'][1] <= origin[1] < w['bounds'][1] + w['bounds'][3]]
        require(len(windows) == 1, {'reason': 'Web surface window is ambiguous', 'windows': windows})
        main = windows[0]['bounds']
        require(abs(width - main[2]) <= 1 and height <= main[3],
                {'reason': 'Retained WebKit viewport no longer matches its window', 'viewport': [width, height], 'window': main})
        offset = {'Target quantity': (40, -78), 'Handoff note': (0, 50),
                  'Reviewed': (-218, -39)}[label]
        screen = [origin[0] + width / 2 + offset[0], origin[1] + height / 2 + offset[1]]
        return image_point(view, screen)

    def _web_observation(self, predicate):
        return wait_until(lambda: o if predicate(o := self.fixture.web_state().get('observation', {})) else None)

    def _web_review_date(self):
        """Enter the observed YYYY/MM/DD widget through real segment keystrokes.

        The original W04 AX frame exposes year/month/day as unknown-role, read-only
        children, not text fields. typeText uses paste and cannot fill this widget.
        Use one-digit month/day to avoid filling their full numeric width before
        the explicit Tab. Public AX may omit these segments even after successful
        input, so complete all keys before requiring the independent date oracle
        to match exactly; an unexpected locale or navigation fails before save.
        """
        expected = '2027-01-02'
        before = self.fixture.web_state()['observation']['review']['values']['date']
        require(before == '', {'reason': 'W04 date must start empty', 'date': before})
        self._web_field('Review date')  # Published anchor is inside the year segment.
        for digits in ('2027', '1', '2'):
            self.run(' '.join('await app.keypress(' + json.dumps(key) + ');' for key in digits))
            self.run("await app.keypress('TAB');")
        self.capture('W04-date-entered')
        observed = self._web_observation(lambda o: o.get('review', {}).get('values', {}).get('date') == expected)
        return expected, observed

    def W04(self):
        """Native HTML rejection, one valid save, then GUI reopen with exact fields."""
        self.web_ready()
        before = self.fixture.web_state()
        require(not before['records'], 'W04 requires a fresh fixture')
        self._web_click('New review record')
        self._web_observation(lambda o: o.get('review', {}).get('open'))
        self._web_click('Save review')
        invalid = self._web_observation(lambda o: o.get('review', {}).get('invalid', 0) > 0)
        require(self.fixture.web_state()['records'] == [], 'Invalid form saved a record')
        require(invalid['review']['open'], invalid)
        self.capture('W04-invalid-form')
        self._web_type('Review note', 'W04 review 中文 café 🙂')
        self._web_type('Review quantity', '37')
        self._web_field('Approved')
        # Keep the default Normal priority; every required date segment is entered.
        try:
            requested_date, observed = self._web_review_date()
        except AssertionError as error:
            self.capture('W04-date-incomplete')
            return {'blocked': 'W04 exact date entry did not verify', 'detail': str(error),
                    'observation': self.fixture.web_state().get('observation'), 'records': self.fixture.web_state()['records']}
        fields = observed['review']['values']
        expected = {'note': 'W04 review 中文 café 🙂', 'number': 37, 'approved': True,
                    'priority': 'Normal', 'date': requested_date}
        require(fields == {**expected, 'number': '37', 'approved': 'on'}, fields)
        self._web_click('Save review')
        after = wait_until(lambda: s if (s := self.fixture.web_state())['records'] else None)
        require(after['records'] == [expected], after['records'])
        self._web_observation(lambda o: not o.get('review', {}).get('open', True))
        self._web_click('New review record')
        reopened = self._web_observation(lambda o: o.get('review', {}).get('open'))
        require(reopened['review']['values'] == fields, reopened)
        self.capture('W04-reopened')
        self._web_click('Cancel')
        self._web_observation(lambda o: not o.get('review', {}).get('open', True))
        require(self.fixture.web_state()['records'] == [expected], 'Reopening caused another save')
        return {'record': expected, 'invalid_observation': invalid, 'reopened': reopened, 'saves_delta': 1}

    def W05(self):
        """Inner vertical and outer horizontal scroll with exact neighbour isolation."""
        self.web_ready()
        self._web_click('Load noisy catalogue')
        wait_until(lambda: self.fixture.web_state()['catalogue_loaded'])
        # Put the queue in view using the page's ordinary focused control.
        self._web_field('Search catalogue')
        baseline = self._web_observation(lambda o: bool(o.get('scrolls')))
        proofs = []
        for name, region, axis in [('scroll-inner', 'inner', 1), ('scroll-outer', 'outer', 0)]:
            point = self._web_point(name, 'W05-' + region)
            before = copy.deepcopy(self.fixture.web_state()['observation']['scrolls'])
            args = {**point, 'delta_y': 8 if axis else 0}
            if not axis:
                args['delta_x'] = 8
            self.run('await app.scroll(' + json.dumps(args) + ');')
            after = self._web_observation(lambda o: o.get('scrolls', {}).get(region, before[region])[axis] != before[region][axis])['scrolls']
            require(after[region][axis] > before[region][axis], {'before': before, 'after': after})
            require(after[region][1-axis] == before[region][1-axis], after)
            require(all(after[k] == before[k] for k in before if k != region), {'before': before, 'after': after})
            proofs.append({'region': region, 'before': before, 'after': after})
        return {'baseline': baseline, 'scrolls': proofs}

    def W06(self):
        """Canvas-only context menu pin plus one Unicode reply to release-137."""
        self.web_ready()
        before = self.fixture.web_state()['chat']
        require(not before['actions'], 'W06 requires a fresh fixture')
        self._web_click('Canvas chat')
        text = 'W06 reply 中文 café العربية 🙂'
        for action in ('pin', 'reply'):
            point = self._web_point('canvas-message-137', 'W06-message-' + action)
            self.run('await app.click(' + json.dumps({**point, 'button': 'right'}) + ');')
            menu = self._web_point('canvas-' + action, 'W06-menu-' + action)
            # Pixel menu must not expose the answer as an AX menu item.
            ax = self.run("const elements=[]; for(const text of ['Release check 137','Pin','Edit','Reply']){"
                          'const v=await app.look({text}); elements.push(...v.elements); } return {elements};')
            require(not any(e.get('name') in ('Pin', 'Edit', 'Reply', 'Release check 137') for e in ax['result']['elements']), ax)
            self.run('await app.click(' + json.dumps(menu) + ');')
            if action == 'pin':
                wait_until(lambda: len(self.fixture.web_state()['chat']['actions']) == 1)
            else:
                point = self._web_point('canvas-compose', 'W06-composer')
                self.run('await app.click(' + json.dumps(point) + "); await app.typeText(" + json.dumps(text) + ');')
                point = self._web_point('canvas-send', 'W06-send')
                self.run('await app.click(' + json.dumps(point) + ');')
        after = wait_until(lambda: c if len((c := self.fixture.web_state()['chat'])['actions']) >= 2 else None)
        require(after['actions'] == [{'action': 'pin', 'id': 'release-137'},
                                    {'action': 'reply', 'id': 'release-137', 'text': text}], after)
        expected = copy.deepcopy(before['messages'])
        next(m for m in expected if m['id'] == 'release-137')['pinned'] = True
        expected.append({'id': 'sent-2', 'text': text, 'pinned': False, 'reply_to': 'release-137'})
        require(after['messages'] == expected, after)
        return {'actions': after['actions'], 'messages_delta': 1, 'input': 'canvas pixels and GUI composer'}

    def _workbench_state(self):
        return json.loads((self.fixture.output / 'workbench/state.json').read_text(encoding='utf-8'))

    def _web_read_field(self, label):
        result = self.run('const v=await app.look({text:' + json.dumps(label) + '});'
            'const e=v.elements.filter(e=>e.name===' + json.dumps(label) +
            '&&e.role===' + json.dumps('text_area' if label == 'Handoff note' else 'text_field') + ');'
            "if(e.length!==1)throw Error('Expected one field');"
            'return await app.readText({target:e[0].id,offset:0,limit:4096});')['result']
        require(result['next_offset'] is None, result)
        return result['text']

    def B02(self):
        """Original Inventory: two actual pages, one save and reopen per SKU."""
        self.web_ready()
        if not self.fixture.workbench_url:
            return {'unavailable': 'Original browser-workbench server is not available'}
        wait_until(lambda: self.fixture.web_state().get('layout', {}).get('viewport'))
        self._web_click('Inventory')
        before = wait_until(lambda: s if (s := self._workbench_state())['ready'] else None)
        require(not before['saves'], 'B02 requires a fresh fixture')
        expected = copy.deepcopy(before['items'])
        reopened = []
        for sku, quantity, page in [('DTW-0088', 37, 1), ('DTW-0240', 52, 3)]:
            if page == 3:
                for target_page in (2, 3):
                    self._web_click('Next page')
                    wait_until(lambda: any(e.get('type') == 'page' and e.get('page') == target_page for e in self._workbench_state()['events']))
            self._web_click('Edit ' + sku)
            note = 'B02 ' + sku + ' 中文🙂'
            self._web_type('Target quantity', str(quantity))
            self._web_field('Reviewed')
            self._web_type('Handoff note', note)
            self._web_click('Save changes')
            count = len(reopened) + 1
            saved = wait_until(lambda: s if len((s := self._workbench_state())['saves']) >= count else None)
            item = next(i for i in expected if i['sku'] == sku)
            item.update(quantity=quantity, reviewed=True, note=note)
            require(saved['items'] == expected and len(saved['saves']) == count, saved)
            require({k: v for k, v in saved['saves'][-1].items() if k != 'unix_ms'} == item, saved['saves'])
            self._web_click('Edit ' + sku)
            require(self._web_read_field('Target quantity') == str(quantity), sku)
            require(self._web_read_field('Handoff note') == note, sku)
            self.capture('B02-reopened-' + sku)
            self._web_click('Cancel')
            reopened.append(sku)
        after = self._workbench_state()
        require(after['items'] == expected and len(after['saves']) == 2, after)
        return {'reopened': reopened, 'pages': [1, 3], 'saves': after['saves'], 'neighbours_unchanged': True}

    def B03(self):
        """Repeated catalogue labels refuse ambiguous input; review only ledger 137."""
        self.web_ready()
        before = self.fixture.web_state()
        require(not before['catalogue_reviews'], 'B03 requires a fresh fixture')
        self._web_click('Load noisy catalogue')
        wait_until(lambda: self.fixture.web_state()['catalogue_loaded'])
        data, response = self.client.execute(self.app,
            "await app.click({selector:{role:'button',text:'Reviewed'}});", allow_error=True)
        require(response.get('isError') and data.get('failure', {}).get('code') == 'selector_ambiguous', data)
        effects = data.get('effects', {})
        require(effects.get('dispatch') == {'not_dispatched': 1} and not effects.get('focus_preparation'), data)
        require(isinstance(data.get('execution_id'), str) and data['execution_id'], data)
        require(self.fixture.web_state()['catalogue_reviews'] == {}, 'Ambiguous input mutated a ledger')
        self._web_type('Search catalogue', 'Audit checkpoint 137')
        # The normal search leaves the intended row as the sole exact label match.
        self._web_click('Reviewed')
        after = wait_until(lambda: s if (s := self.fixture.web_state())['catalogue_reviews'] else None)
        require(after['catalogue_reviews'] == {'137': True}, after['catalogue_reviews'])
        events = [json.loads(line) for line in (self.fixture.output / 'web/events.jsonl').read_text().splitlines()]
        writes = [e for e in events if e['type'] == '/api/catalogue-review']
        require(len(writes) == 1 and writes[0]['data'] == {'id': 137}, writes)
        require(after['records'] == before['records'] and after['saves'] == before['saves'], after)
        self.capture('B03-reviewed-137')
        return {'ambiguous_failure': data['failure'], 'refusal_effects': effects,
                'expected_tool_errors': [data['execution_id']],
                'reviews': after['catalogue_reviews'], 'writes': writes}
