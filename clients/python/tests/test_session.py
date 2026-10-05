import asyncio
import os
import unittest
from desktop_world import HostSession, DesktopError, known

class SessionTests(unittest.IsolatedAsyncioTestCase):
    async def test_native_owner_and_receipts(self):
        helper = os.environ['DTW_SDK_TEST_HELPER']
        async with await HostSession.start(helper, input_mode='shared', write_apps=['Later']) as host:
            dw = host.desktop
            ob = await dw.observe({'projection':'outline', 'fields':['name','role','app','window','states']})
            refs = {known(o['name']):o for o in ob['objects'] if 'name' in o and o['name'].get('status')=='known'}
            self.assertTrue(ob['coverage']['complete'])
            self.assertIs(known(refs['内容']['states']['checked']), False)
            self.assertIsInstance(refs['内容']['version'], str)
            self.assertEqual((await host.grants())['grants'][0]['state'], 'pending')
            with self.assertRaises(ValueError): await dw.call('grant', {'application':refs['Fixture']['ref']})
            with self.assertRaises(DesktopError): await dw.set(refs['内容']['ref'], 'denied', request_id='denied')
            await host.grant(refs['Fixture']['ref'])
            await host.grant(refs['Other']['ref'])
            plan = dw.plan().set(refs['内容']['ref'], 'SDK 中文\n🙂')
            receipt = await dw.act(plan, request_id='original')
            self.assertEqual((await dw.act(plan, request_id='original'))['run_id'], receipt['run_id'])
            with self.assertRaises(DesktopError) as conflict: await dw.set(refs['内容']['ref'], 'other', request_id='original')
            self.assertEqual(conflict.exception.code, 'request_conflict')
            self.assertEqual(known((await dw.read(refs['内容']['ref']))['text']), 'SDK 中文\n🙂')
            self.assertEqual((await dw.reconcile('original'))['result']['run_id'], receipt['run_id'])
            plan = dw.plan().focus(refs['内容']['ref'])
            plan.press(plan.bind_focus('input', refs['Desktop World Fixture']['ref']), 'A', ['primary'])
            await dw.act(plan)
            await host.revoke(refs['Fixture']['ref'])
            with self.assertRaises(DesktopError): await dw.set(refs['内容']['ref'], 'blocked')
            await dw.set(refs['Other Field']['ref'], '')
            self.assertEqual(known((await dw.read(refs['Other Field']['ref']))['text']), '')
            await host.grant(refs['Fixture']['ref'])
            task=asyncio.create_task(dw.invoke(refs['提交']['ref'],request_id='cancel-original'))
            await asyncio.sleep(.05)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError): await task
            stopped=await asyncio.wait_for(dw.reconcile('cancel-original'),2)
            self.assertIn('run_id',stopped['result'])
            self.assertEqual((await dw.get(receipt['run_id']))['run_id'],receipt['run_id'])
            with self.assertRaises(DesktopError): await dw.observe()
            await host.begin_turn('fresh')
            self.assertEqual((await host.grants())['grants'], [])

    def test_false_empty_unknown(self):
        self.assertIs(known({'status':'known','value':False}), False)
        self.assertEqual(known({'known':''}), '')
        with self.assertRaises(DesktopError): known({'status':'redacted'})
if __name__=='__main__': unittest.main()
