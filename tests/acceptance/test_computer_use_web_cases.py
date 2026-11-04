"""Supervisor contract checks; these do not claim a GUI acceptance pass."""
import copy
import json
import re
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock

from computer_use_web_cases import WebExtendedCases


class WebHarness(WebExtendedCases):
    def __init__(self, output):
        self.web = {'records': [], 'saves': [], 'catalogue_loaded': False, 'catalogue_reviews': {},
                    'layout': {'viewport': [1000, 700]},
                    'observation': {'review': {'open': False, 'invalid': 0, 'values':
                        {'note': '', 'number': '', 'approved': '', 'priority': 'Normal', 'date': ''}},
                        'scrolls': {'inner': [0, 0], 'outer': [0, 0], 'document': [0, 300]}}}
        self.fixture = SimpleNamespace(web_state=lambda: copy.deepcopy(self.web), output=output, workbench_url=None)
        self.client = SimpleNamespace(execute=self.execute)
        self.app = 'owned-fixture'
        self.actions = []
        self.date_segments = {'year': '', 'month': '', 'day': ''}
        self.date_focus = None
        self.date_keys_delivered = True
        self.date_dom_value = '2027-01-02'
        self.scroll_leak = False
        self.invalid_saves = False
        self.ambiguous_write = False
        self.ambiguous_code = 'selector_ambiguous'
        self.refusal_effects = {'dispatch': {'not_dispatched': 1}}
        (output / 'web').mkdir()
        (output / 'web/events.jsonl').touch()

    def web_ready(self):
        self.actions.append('web_ready')

    def capture(self, name):
        self.actions.append(name)

    def _web_point(self, name, capture):
        return {'x': 10 if name == 'scroll-inner' else 20, 'y': 10}

    def _web_click(self, label):
        self.actions.append(label)
        review = self.web['observation']['review']
        if label == 'New review record':
            review['open'] = True
        elif label == 'Save review':
            if not review['values']['note']:
                review['invalid'] += 1
                if self.invalid_saves:
                    self.web['records'].append({'invalid': True})
            else:
                values = review['values']
                self.web['records'].append({**values, 'number': int(values['number']), 'approved': True})
                review['open'] = False
        elif label == 'Approved':
            review['values']['approved'] = 'on'
        elif label == 'Cancel':
            review['open'] = False
        elif label == 'Load noisy catalogue':
            self.web['catalogue_loaded'] = True
        elif label == 'Reviewed':
            self.web['catalogue_reviews'] = {'137': True}
            with (self.fixture.output / 'web/events.jsonl').open('a') as stream:
                stream.write(json.dumps({'type': '/api/catalogue-review', 'data': {'id': 137}}) + '\n')

    def _web_field(self, label):
        if label == 'Review date':
            self.date_focus = 'year'
        return self._web_click(label)

    def _web_type(self, label, value):
        self.actions.append((label, value))
        key = {'Review note': 'note', 'Review quantity': 'number'}.get(label)
        if key:
            self.web['observation']['review']['values'][key] = value

    def run(self, source):
        self.actions.append(source)
        if 'app.look({text:' in source:
            return {'result': {'elements': []}}
        for key in re.findall(r'app.keypress\([\'\"]([^\'\"]+)[\'\"]\)', source):
            if self.date_focus and self.date_keys_delivered:
                if key.isdigit():
                    self.date_segments[self.date_focus] += key
                elif key == 'TAB':
                    self.date_focus = {'year': 'month', 'month': 'day', 'day': None}[self.date_focus]
                    if self.date_focus is None:
                        self.web['observation']['review']['values']['date'] = self.date_dom_value
        if 'app.scroll(' in source:
            args = json.loads(source.split('app.scroll(', 1)[1].split(');', 1)[0])
            region, axis = ('inner', 1) if args['x'] == 10 else ('outer', 0)
            self.web['observation']['scrolls'][region][axis] += 8
            if self.scroll_leak:
                self.web['observation']['scrolls']['document'][1] += 1

    def execute(self, app, source, **kwargs):
        self.actions.append(source)
        if self.ambiguous_write:
            self.web['catalogue_reviews']['136'] = True
        return {'failure': {'code': self.ambiguous_code}, 'effects': self.refusal_effects,
                'execution_id': 'exec-exact-refusal'}, {'isError': True}


class CanvasHarness(WebHarness):
    def __init__(self, output):
        super().__init__(output)
        self.web['chat'] = {'messages': [
            {'id': 'release-137', 'text': 'Release check 137', 'pinned': False},
            {'id': 'neighbour', 'text': 'Preserve this message', 'pinned': False}], 'actions': []}
        self.ax_menu = False
        self.corrupt_neighbour = False

    def _web_point(self, name, capture):
        return {'x': {'canvas-message-137': 1, 'canvas-pin': 2, 'canvas-reply': 3,
                      'canvas-compose': 4, 'canvas-send': 5}[name], 'y': 10}

    def run(self, source):
        self.actions.append(source)
        chat = self.web['chat']
        if 'app.look' in source:
            return {'result': {'elements': [{'name': 'Pin'}] if self.ax_menu else []}}
        if 'app.typeText(' in source:
            self.reply = json.loads(source.split('app.typeText(', 1)[1].split(');', 1)[0])
        if 'app.click(' in source:
            point = json.loads(source.split('app.click(', 1)[1].split(');', 1)[0])
            if point['x'] == 2:
                chat['actions'].append({'action': 'pin', 'id': 'release-137'})
                chat['messages'][0]['pinned'] = True
            elif point['x'] == 5:
                chat['actions'].append({'action': 'reply', 'id': 'release-137', 'text': self.reply})
                chat['messages'].append({'id': 'sent-2', 'text': self.reply, 'pinned': False, 'reply_to': 'release-137'})
                if self.corrupt_neighbour:
                    chat['messages'][1]['text'] = 'Unexpected edit'


class InventoryHarness(WebHarness):
    def __init__(self, output):
        super().__init__(output)
        self.fixture.workbench_url = 'http://127.0.0.1:12345/'
        self.inventory = {'ready': True, 'saves': [], 'events': [], 'items': [
            {'sku': sku, 'quantity': 10, 'reviewed': False, 'note': ''}
            for sku in ('DTW-0088', 'DTW-0089', 'DTW-0240')]}
        self.page = 1

    def _workbench_state(self):
        return copy.deepcopy(self.inventory)

    def _web_click(self, label):
        self.actions.append(label)
        if label.startswith('Edit '):
            self.editing = next(i for i in self.inventory['items'] if i['sku'] == label[5:])
            self.draft = copy.deepcopy(self.editing)
        elif label == 'Reviewed':
            self.draft['reviewed'] = True
        elif label == 'Save changes':
            self.editing.update(self.draft)
            self.inventory['saves'].append({**self.draft, 'unix_ms': 123})
        elif label == 'Next page':
            self.page += 1
            self.inventory['events'].append({'type': 'page', 'page': self.page})

    def _web_type(self, label, value):
        if label == 'Target quantity':
            self.draft['quantity'] = int(value)
        else:
            self.draft['note'] = value

    def _web_read_field(self, label):
        return str(self.draft['quantity']) if label == 'Target quantity' else self.draft['note']


class WebInputTests(unittest.TestCase):
    def setUp(self):
        self.case = WebExtendedCases()
        self.case.run = Mock()
        self.case.capture = Mock(return_value={'image': {'bounds': [-100, 0, 1300, 900], 'width': 2600, 'height': 1800}})
        self.case.state = Mock(return_value={'anchors': {'web-origin': [100, 125]},
            'windows': [{'bounds': [100, 100, 1000, 800]}]})
        self.case.fixture = SimpleNamespace(web_state=lambda: {'layout': {'viewport': [1000, 700],
            'anchors': {'review-note': [300, 200], 'review-approved': [25, 40],
                        'review-date': [300, 300], 'catalogue-search': [250, 600]}}})

    def test_links_and_buttons_have_explicit_roles(self):
        for label, role in [('Canvas chat', 'link'), ('Inventory', 'link'),
                            ('Save review', 'button'), ('Reviewed', 'button')]:
            self.case._web_click(label)
            args = json.loads(self.case.run.call_args.args[0].split('app.click(', 1)[1].split(');')[0])
            self.assertEqual(args['selector'], {'role': role, 'text': label})

    def test_nonpress_field_typing_uses_current_capture_point(self):
        self.case.run.return_value = {'result': {'elements': [{'id': 'w1.e1', 'role': 'text_field',
            'name': 'Review note', 'actions': ['setValue']}]}}
        self.case._web_type('Review note', '中文🙂')
        source = self.case.run.call_args_list[1].args[0]
        self.assertEqual(source, 'await app.click({"x": 1000.0, "y": 650.0});')
        self.assertNotIn('selector', source)
        self.assertIn('app.typeText', self.case.run.call_args_list[2].args[0])
        self.case.capture.assert_called_once_with('web-field-review-note')

    def test_observed_press_field_is_used_once_without_pixel_replay(self):
        observed = {'result': {'elements': [{'id': 'w1.e7', 'role': 'text_field',
            'name': 'Search catalogue', 'actions': ['press', 'setValue']}]}}
        self.case.run.return_value = observed
        self.case._web_type('Search catalogue', '137')
        self.assertEqual(self.case.run.call_args_list[1].args[0], 'await app.click({target:"w1.e7"});')
        self.case.capture.assert_not_called()
        self.case.run.reset_mock()
        self.case.run.side_effect = [observed, AssertionError('unknown delivery')]
        with self.assertRaisesRegex(AssertionError, 'unknown delivery'):
            self.case._web_type('Search catalogue', '137')
        self.assertEqual(self.case.run.call_count, 2)
        self.case.capture.assert_not_called()

    def test_owned_checkbox_date_search_also_use_pixels(self):
        for label in ('Approved', 'Review date', 'Search catalogue'):
            self.case._web_field(label)
            self.assertNotIn('selector', self.case.run.call_args.args[0])

    def test_workbench_field_points_use_web_viewport_not_composite_png_center(self):
        point = self.case._workbench_field_point('Target quantity')
        self.assertEqual(point, {'x': 1480.0, 'y': 794.0})
        note = self.case._workbench_field_point('Handoff note')
        self.assertEqual(note, {'x': 1400.0, 'y': 1050.0})
        checkbox = self.case._workbench_field_point('Reviewed')
        self.assertEqual(checkbox, {'x': 964.0, 'y': 872.0})

    def test_workbench_rejects_stale_viewport_or_ambiguous_surface(self):
        state = self.case.state.return_value
        state['windows'][0]['bounds'][2] = 900
        with self.assertRaises(AssertionError):
            self.case._workbench_field_point('Target quantity')
        state['windows'][0]['bounds'][2] = 1000
        state['windows'].append(copy.deepcopy(state['windows'][0]))
        with self.assertRaises(AssertionError):
            self.case._workbench_field_point('Target quantity')

    def test_reopened_workbench_fields_filter_the_actual_editable_role(self):
        self.case.run.return_value = {'result': {'text': '37', 'next_offset': None}}
        self.assertEqual(self.case._web_read_field('Target quantity'), '37')
        self.assertIn('e.role==="text_field"', self.case.run.call_args.args[0])
        self.case._web_read_field('Handoff note')
        self.assertIn('e.role==="text_area"', self.case.run.call_args.args[0])


class WebCasesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.case = WebHarness(Path(self.temp.name))

    def test_modal_rejects_then_saves_once_and_reopens(self):
        result = self.case.W04()
        self.assertEqual(result['saves_delta'], 1)
        self.assertEqual(len(self.case.web['records']), 1)
        self.assertEqual(result['record']['note'], 'W04 review 中文 café 🙂')
        self.assertEqual(result['record']['date'], '2027-01-02')
        self.assertEqual(result['reopened']['review']['values']['date'], '2027-01-02')
        actions = self.case.actions
        self.assertLess(actions.index('Save review'), actions.index(('Review note', 'W04 review 中文 café 🙂')))
        self.assertEqual(actions.count('New review record'), 2)
        self.assertEqual(actions.count('Save review'), 2)
        self.assertEqual(actions[0], 'web_ready')

    def test_date_recipe_requires_independent_date_oracle_before_save(self):
        from unittest.mock import patch
        self.case.date_keys_delivered = False
        def observe(predicate):
            observation = copy.deepcopy(self.case.web['observation'])
            if not predicate(observation):
                raise AssertionError('requested date not observed')
            return observation
        with patch.object(self.case, '_web_observation', side_effect=observe):
            result = self.case.W04()
        self.assertIn('blocked', result)
        self.assertIn('requested date not observed', result['detail'])
        self.assertEqual(self.case.web['records'], [])
        self.assertEqual(self.case.actions.count('Review date'), 1)
        self.assertEqual(self.case.actions.count('Save review'), 1)
        self.assertIn('W04-date-entered', self.case.actions)
        self.assertIn('W04-date-incomplete', self.case.actions)

    def test_date_recipe_rejects_other_valid_date_and_never_replays(self):
        from unittest.mock import patch
        self.case.date_dom_value = '2027-01-03'
        def observe(predicate):
            observation = copy.deepcopy(self.case.web['observation'])
            if not predicate(observation):
                raise AssertionError('requested date not observed')
            return observation
        with patch.object(self.case, '_web_observation', side_effect=observe):
            result = self.case.W04()
        self.assertIn('blocked', result)
        self.assertEqual(self.case.actions.count('Review date'), 1)
        self.assertEqual(self.case.actions.count('Save review'), 1)
        self.assertEqual(self.case.web['records'], [])

    def test_date_recipe_rejects_prefill_without_sending_input(self):
        self.case.web['observation']['review']['values']['date'] = '2027-01-02'
        with self.assertRaisesRegex(AssertionError, 'must start empty'):
            self.case._web_review_date()
        self.assertEqual(self.case.actions, [])

    def test_date_recipe_saves_with_no_public_segment_ax_values(self):
        self.assertEqual(self.case.run('const v=await app.look({text:"year"}); return {elements:v.elements};'),
                         {'result': {'elements': []}})
        self.case.actions.clear()
        result = self.case.W04()
        self.assertEqual(result['record']['date'], '2027-01-02')
        self.assertEqual(self.case.web['records'], [result['record']])
        self.assertFalse(any('app.look' in action for action in self.case.actions if isinstance(action, str)))

    def test_date_recipe_uses_digit_key_events_and_no_paste(self):
        self.case._web_review_date()
        scripts = [action for action in self.case.actions if isinstance(action, str) and 'app.' in action]
        keys = [key for script in scripts for key in re.findall(r'app.keypress\([\'\"]([^\'\"]+)[\'\"]\)', script)]
        self.assertEqual(keys, ['2', '0', '2', '7', 'TAB', '1', 'TAB', '2', 'TAB'])
        self.assertFalse(any('typeText' in script or 'setValue' in script or 'app.look' in script
                             for script in scripts))
        self.assertEqual(self.case.date_segments, {'year': '2027', 'month': '1', 'day': '2'})

    def test_modal_cannot_pass_if_invalid_attempt_writes(self):
        self.case.invalid_saves = True
        with self.assertRaises(AssertionError):
            self.case.W04()

    def test_nested_scroll_proves_both_axes_and_exact_neighbours(self):
        result = self.case.W05()
        self.assertEqual([p['region'] for p in result['scrolls']], ['inner', 'outer'])
        self.assertEqual(result['scrolls'][1]['after']['inner'], [0, 8])
        self.assertEqual(result['scrolls'][1]['after']['outer'], [8, 0])

    def test_scroll_rejects_even_one_pixel_of_neighbour_movement(self):
        self.case.scroll_leak = True
        with self.assertRaises(AssertionError):
            self.case.W05()

    def test_duplicate_label_refusal_and_exact_row_mutation(self):
        result = self.case.B03()
        self.assertEqual(result['reviews'], {'137': True})
        self.assertEqual(len(result['writes']), 1)
        self.assertEqual(result['expected_tool_errors'], ['exec-exact-refusal'])
        self.assertIn(('Search catalogue', 'Audit checkpoint 137'), self.case.actions)

    def test_ambiguous_input_must_not_mutate_any_row(self):
        self.case.ambiguous_write = True
        with self.assertRaises(AssertionError):
            self.case.B03()

    def test_unrelated_tool_error_cannot_count_as_ambiguity_refusal(self):
        self.case.ambiguous_code = 'foreground_required'
        with self.assertRaises(AssertionError):
            self.case.B03()

    def test_ambiguity_cannot_prepare_focus_or_dispatch_input(self):
        for effects in ({'dispatch': {'dispatched': 1}},
                        {'dispatch': {'not_dispatched': 1}, 'focus_preparation': {'attempted': 1}}):
            self.case.refusal_effects = effects
            with self.assertRaises(AssertionError):
                self.case.B03()

    def test_canvas_pin_reply_and_unicode_preserve_every_neighbour(self):
        with tempfile.TemporaryDirectory() as output:
            case = CanvasHarness(Path(output))
            result = case.W06()
            self.assertEqual([a['action'] for a in result['actions']], ['pin', 'reply'])
            self.assertEqual(result['actions'][1]['text'], 'W06 reply 中文 café العربية 🙂')
            self.assertEqual(case.actions[0], 'web_ready')

    def test_canvas_rejects_ax_menu_answer_and_neighbour_edits(self):
        for fault in ('ax_menu', 'corrupt_neighbour'):
            with tempfile.TemporaryDirectory() as output:
                case = CanvasHarness(Path(output))
                setattr(case, fault, True)
                with self.assertRaises(AssertionError):
                    case.W06()

    def test_inventory_uses_actual_pages_two_saves_and_two_reopens(self):
        with tempfile.TemporaryDirectory() as output:
            case = InventoryHarness(Path(output))
            result = case.B02()
            self.assertEqual(result['reopened'], ['DTW-0088', 'DTW-0240'])
            self.assertEqual(case.actions.count('Next page'), 2)
            self.assertEqual(case.actions.count('Save changes'), 2)
            self.assertEqual(case.actions.count('Edit DTW-0088'), 2)
            self.assertEqual(case.actions.count('Edit DTW-0240'), 2)
            self.assertEqual(case.inventory['items'][1], {'sku': 'DTW-0089', 'quantity': 10, 'reviewed': False, 'note': ''})

    def test_original_workbench_unavailable_is_explicit(self):
        result = self.case.B02()
        self.assertTrue(result['unavailable'])
        self.assertEqual(self.case.actions, ['web_ready'])


if __name__ == '__main__':
    unittest.main()
