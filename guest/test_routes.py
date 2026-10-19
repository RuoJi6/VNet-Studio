import importlib.util
import pathlib
import unittest
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('routes',pathlib.Path(__file__).with_name('routes.py'))
routes=importlib.util.module_from_spec(spec);spec.loader.exec_module(routes)

def state():
    return dict(fingerprint='abc',interfaces=[dict(id='wan0',ips=['10.0.0.2/24'],state='UP'),dict(id='lan0',ips=['192.168.8.2/24'],state='UP')],routes=[dict(destination='0.0.0.0/0',gateway='10.0.0.1',interface='wan0',metric=100,protocol='dhcp',scope='global',protected=False)])

class RouteTests(unittest.TestCase):
    def test_default_preview_does_not_run_commands(self):
        with patch.object(routes,'run') as run:
            p=routes.plan(dict(op='default',interface='wan0',gateway='10.0.0.1'),state())
            self.assertEqual(p['after'][0]['metric'],0)
            self.assertEqual(p['before'][0]['protocol'],'dhcp')
            run.assert_not_called()
    def test_ip_is_host_route(self):
        p=routes.plan(dict(op='add',destination='192.168.8.40',interface='lan0',gateway=''),state())
        self.assertEqual(p['destination'],'192.168.8.40/32')
    def test_nonlocal_gateway_rejected(self):
        with self.assertRaisesRegex(ValueError,'网关必须'):
            routes.plan(dict(op='default',interface='lan0',gateway='10.0.0.1'),state())
    def test_remote_target_requires_gateway(self):
        with self.assertRaisesRegex(ValueError,'下一跳'):
            routes.plan(dict(op='add',destination='203.0.113.0/24',interface='wan0',gateway=''),state())
    def test_connected_or_complex_routes_protected(self):
        s=state();s['routes'][0]['protected']=True
        with self.assertRaisesRegex(ValueError,'复杂路由'):
            routes.plan(dict(op='default',interface='wan0',gateway='10.0.0.1'),s)
    def test_stale_plan_fails_before_mutation(self):
        with patch.object(routes,'snapshot',return_value=state()),patch.object(routes,'replace_rows') as mutate:
            with self.assertRaisesRegex(RuntimeError,'变化'):routes.main(dict(action='apply',request=dict(fingerprint='wrong')))
            mutate.assert_not_called()
    def test_replace_failure_restores_deleted_route(self):
        s=state();old=s['routes'];new=[dict(old[0],interface='lan0',gateway='192.168.8.1')]
        with patch.object(routes,'run',side_effect=['',RuntimeError('fail'),'']) as run:
            with self.assertRaisesRegex(RuntimeError,'回滚：成功'):routes.replace_rows(old,new,False)
            self.assertEqual(run.call_args_list[-1].args[0],routes.route_args('add',old[0],False))
    def test_mac_scoped_default_is_not_rewritten(self):
        rows=routes.mac_routes('default 10.0.0.1 UGSc en0\ndefault 192.168.8.1 UGScI en1\n10/24 link#2 UCS en0\n10.0.0.2 aa:bb UHLWI en0')
        selected=routes.affected(dict(routes=rows),'0.0.0.0/0')
        self.assertEqual(len(selected),1)
        self.assertEqual(selected[0]['interface'],'en0')
        self.assertTrue(rows[-1]['protected'])
    def test_target_normalization(self):
        self.assertEqual(routes.prefix('192.168.10.117/24'),'192.168.10.0/24')
        for s in ['127.0.0.1','224.1.1.1','bad','0.0.0.1']:
            with self.assertRaises(ValueError):routes.ipv4(s)

if __name__=='__main__':unittest.main()
