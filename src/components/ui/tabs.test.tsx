import { describe, it } from 'vitest'
import { render } from '@testing-library/react'
import { Tabs, TabsList, TabsTrigger } from './tabs'

describe('Tabs', () => {
  it('renders without crashing', () => {
    render(
      <Tabs value="list">
        <TabsList>
          <TabsTrigger value="list">List</TabsTrigger>
          <TabsTrigger value="grid">Grid</TabsTrigger>
        </TabsList>
      </Tabs>
    )
  })
})
