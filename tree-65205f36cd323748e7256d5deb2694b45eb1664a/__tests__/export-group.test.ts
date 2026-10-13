import {describe, expect, it} from 'vitest'
import {validateExportGroup} from '../src/export-group.js'

describe('owner export group', () => {
  it('preserves a canonical caller group and the default unique-group path', () => {
    expect(validateExportGroup('velnor-fixture_v1.2', 'github', 'objects')).toBe('velnor-fixture_v1.2')
    expect(validateExportGroup('a'.repeat(256), 'github', 'objects')).toBe('a'.repeat(256))
    expect(validateExportGroup('', 'local', 'target')).toBe('')
  })
  it.each([
    'velnor-mbx-b3-554e69d4f330d0f2fd13e6860553d49303071f172e3b686aadfab7ec3d67ec57-r37012391691-a1',
    'velnor-mbx-b3-554e69d4f330d0f2fd13e6860553d49303071f172e3b686aadfab7ec3d67ec57-r18446744073709551615-a18446744073709551615'
  ])('preserves the complete generated descriptor group %s', group => {
    expect(validateExportGroup(group, 'github', 'objects')).toBe(group)
  })
  it.each(['A', '-group', '../group', 'group/name', 'group name', 'group\n', 'é', 'a'.repeat(257)])('rejects noncanonical group %j', group => {
    expect(() => validateExportGroup(group, 'github', 'objects')).toThrow(/export-group/)
  })
  it.each([['local', 'objects'], ['remote', 'objects'], ['github', 'target']])('rejects incompatible %s/%s mode', (backend, mode) => {
    expect(() => validateExportGroup('group', backend, mode)).toThrow(/requires/)
  })
})
